"""
Claude Limits — мини-виджет для Windows, показывает остаток лимитов Claude
(5-часовое окно и недельное) для аккаунта, залогиненного в Claude Desktop.

Сессия берётся из cookies Claude Desktop (%APPDATA%\\Claude), расшифровывается
локально через DPAPI. Никуда, кроме claude.ai, ничего не отправляется.
Если авто-чтение не сработало — ПКМ по виджету → «Ввести sessionKey».

Обновляется сам каждые 15 с (ПКМ → Автообновление). ЛКМ — перетащить,
двойной клик — обновить сейчас, ПКМ — меню.
Зависимости: pip install cryptography curl_cffi
"""
import os, sys, json, glob, base64, sqlite3, shutil, tempfile, threading, queue
import ctypes, ctypes.wintypes as wt
import urllib.request, urllib.error
from datetime import datetime, timezone
import tkinter as tk
from tkinter import simpledialog

APPDATA = os.environ.get("APPDATA", "")
LOCALAPPDATA = os.environ.get("LOCALAPPDATA", "")
CFG_DIR = os.path.join(APPDATA, "ClaudeLimits")
CFG_PATH = os.path.join(CFG_DIR, "config.json")
BASE = "https://claude.ai/api"
INTERVALS = [5, 15, 30, 60]   # варианты автообновления, секунды
DEFAULT_INTERVAL = 15
MAX_BACKOFF = 300             # при 429 замедляемся до 5 минут
TICK_MS = 10_000              # перерисовка таймеров

ROWS = [  # ключ ответа API -> подпись (первые два показываются всегда)
    ("five_hour", "5 ч"),
    ("seven_day", "Неделя"),
    ("seven_day_sonnet", "Sonnet"),
    ("seven_day_opus", "Opus"),
]

BG, FG, DIM, TRACK = "#1f1e1d", "#f0eee6", "#8f8c84", "#3a3936"
BORDER = "#34322f"
ACCENT = "#d97757"          # тёплый оранжевый
KEY = "#010203"             # цвет-«дырка» для прозрачного фона окна
FONT, FONT_SMALL = ("Segoe UI", 9), ("Segoe UI", 8)
FONT_TITLE = ("Segoe UI Semibold", 9)

W = 252                     # ширина панели
CAT_PX = 2                  # размер «пикселя» котика
TOP = 0                    # сколько котик торчит над панелью
HEADER_H = 26
ROW_H = 21

# ---------------- котик (пиксель-арт) ----------------
# w — белый, g — тень, k — глаза, p — розовый, o — контур (считается сам)
CAT_HEAD = [
    "w.....w",
    "wwwwwww",
    "wkwwwkw",
    "wwwpwww",
    ".wwwww.",
]
CAT_HEAD_BLINK = CAT_HEAD[:2] + ["wgwwwgw"] + CAT_HEAD[3:]
CAT_HEAD_EAR = [".w....w"] + CAT_HEAD[1:]   # ушко дёрнулось
CAT_BODY = [
    ".wwwww.",
    "wwwwwww",
    "wgwwwgw",
]
CAT_TAILS = [   # хвост справа (x, y от верха тела)
    [(7, 2), (8, 1), (8, 0)],
    [(7, 2), (8, 2), (8, 1)],
    [(7, 2), (8, 1), (9, 0)],
    [(7, 2), (8, 1), (8, 0)],
]
CAT_COLORS = {"w": "#ffffff", "g": "#d9d6cf", "k": "#2b2a28", "p": "#f2a7b0", "o": "#141312"}


def cat_frame(t):
    """Пиксели котика для тика анимации t (≈ 8 тиков в секунду)."""
    breathe = 1 if (t // 10) % 2 else 0               # медленное дыхание
    head = CAT_HEAD
    if t % 46 in (0, 1):                               # моргает
        head = CAT_HEAD_BLINK
    elif t % 97 in (30, 31, 33):                       # иногда дёргает ухом
        head = CAT_HEAD_EAR
    tail = CAT_TAILS[(t // 3) % len(CAT_TAILS)]
    px = {}
    for y, row in enumerate(CAT_BODY):
        for x, c in enumerate(row):
            if c != ".":
                px[(x, y + 5)] = c
    for x, y in tail:
        px[(x, y + 5)] = "w"
    for y, row in enumerate(head):
        for x, c in enumerate(row):
            if c != ".":
                px[(x, y + breathe)] = c
    out = {}
    for (x, y) in px:
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            q = (x + dx, y + dy)
            if q not in px:
                out[q] = "o"
    out.update(px)
    return out


def rounded_rect(cv, x0, y0, x1, y1, r, **kw):
    pts = [x0 + r, y0, x1 - r, y0, x1, y0, x1, y0 + r, x1, y1 - r, x1, y1,
           x1 - r, y1, x0 + r, y1, x0, y1, x0, y1 - r, x0, y0 + r, x0, y0]
    return cv.create_polygon(pts, smooth=True, **kw)


def sparkle(cv, cx, cy, r, color, **kw):
    """Простая четырёхлучевая звёздочка."""
    k = r * 0.28
    pts = [cx, cy - r, cx + k, cy - k, cx + r, cy, cx + k, cy + k,
           cx, cy + r, cx - k, cy + k, cx - r, cy, cx - k, cy - k]
    cv.create_polygon(pts, fill=color, outline="", **kw)
    cv.create_oval(cx + r * 0.55, cy - r * 1.05, cx + r * 0.95, cy - r * 0.65,
                   fill=color, outline="", **kw)


# ---------------- DPAPI ----------------
class _BLOB(ctypes.Structure):
    _fields_ = [("cbData", wt.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]


def dpapi(data: bytes, protect=False) -> bytes:
    buf = ctypes.create_string_buffer(data, len(data))
    inp = _BLOB(len(data), ctypes.cast(buf, ctypes.POINTER(ctypes.c_char)))
    out = _BLOB()
    fn = ctypes.windll.crypt32.CryptProtectData if protect else ctypes.windll.crypt32.CryptUnprotectData
    if not fn(ctypes.byref(inp), None, None, None, None, 0, ctypes.byref(out)):
        raise OSError("DPAPI не смог расшифровать данные")
    try:
        return ctypes.string_at(out.pbData, out.cbData)
    finally:
        ctypes.windll.kernel32.LocalFree(out.pbData)


# ---------------- config ----------------
def load_cfg():
    try:
        with open(CFG_PATH, encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return {}


def save_cfg(cfg):
    os.makedirs(CFG_DIR, exist_ok=True)
    with open(CFG_PATH, "w", encoding="utf-8") as f:
        json.dump(cfg, f, indent=1)


# ---------------- cookies Claude Desktop ----------------
def claude_dirs():
    cands = [os.path.join(APPDATA, "Claude")]
    # установка из Microsoft Store / MSIX
    cands += glob.glob(os.path.join(LOCALAPPDATA, "Packages", "*Claude*", "LocalCache", "Roaming", "Claude"))
    return [d for d in cands if os.path.exists(os.path.join(d, "Local State"))]


def _query_cookie_db(path):
    sql = "SELECT name, encrypted_value, value FROM cookies WHERE host_key LIKE '%claude.ai'"
    tmp = os.path.join(tempfile.gettempdir(), "claude_limits_cookies.db")
    try:
        shutil.copyfile(path, tmp)
        con = sqlite3.connect(tmp)
    except OSError:
        # файл занят запущенным Claude — пробуем открыть только на чтение без блокировок
        uri = "file:" + urllib.request.pathname2url(path) + "?mode=ro&immutable=1"
        con = sqlite3.connect(uri, uri=True)
    try:
        try:
            ver = int(con.execute("SELECT value FROM meta WHERE key='version'").fetchone()[0])
        except Exception:
            ver = 0
        return con.execute(sql).fetchall(), ver
    finally:
        con.close()
        try:
            os.remove(tmp)
        except OSError:
            pass


def _decrypt(enc: bytes, key: bytes, db_ver: int) -> str:
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    if enc[:3] in (b"v10", b"v11"):
        pt = AESGCM(key).decrypt(enc[3:15], enc[15:], None)
    elif enc[:3] == b"v20":
        raise RuntimeError("app-bound шифрование cookies не поддерживается")
    else:
        pt = dpapi(enc)
    if db_ver >= 24:      # новые Chromium добавляют SHA256(host) перед значением
        pt = pt[32:]
    return pt.decode("utf-8", "replace")


def read_desktop_cookies(d):
    with open(os.path.join(d, "Local State"), encoding="utf-8") as f:
        ls = json.load(f)
    key = dpapi(base64.b64decode(ls["os_crypt"]["encrypted_key"])[5:])  # срезаем префикс "DPAPI"
    for p in (os.path.join(d, "Network", "Cookies"), os.path.join(d, "Cookies")):
        if os.path.exists(p):
            break
    else:
        raise FileNotFoundError("cookies Claude Desktop не найдены")
    rows, ver = _query_cookie_db(p)
    cookies = {}
    for name, enc, plain in rows:
        try:
            cookies[name] = plain or (_decrypt(enc, key, ver) if enc else "")
        except Exception:
            if name == "sessionKey":
                raise
    if not cookies.get("sessionKey"):
        raise RuntimeError("в Claude Desktop нет активной сессии")
    return cookies


# ---------------- HTTP ----------------
LOG_PATH = os.path.join(CFG_DIR, "log.txt")


def log(msg):
    try:
        os.makedirs(CFG_DIR, exist_ok=True)
        with open(LOG_PATH, "a", encoding="utf-8") as f:
            f.write(f"{datetime.now():%Y-%m-%d %H:%M:%S} {msg}\n")
    except Exception:
        pass


class HttpErr(Exception):
    def __init__(self, code, body=""):
        super().__init__(code)
        self.code = code
        self.body = body or ""

    @property
    def cloudflare(self):
        b = self.body.lower()
        return "cloudflare" in b or "<html" in b or "just a moment" in b


HEADERS = {
    "Accept": "application/json",
    "Referer": "https://claude.ai/",
    "anthropic-client-platform": "web_claude_ai",
}


def http_get(url, cookies):
    try:
        from curl_cffi import requests as cr  # маскируется под Chrome, чтобы не упираться в Cloudflare
    except Exception as e:
        log(f"curl_cffi недоступен: {e!r}")
        cr = None
    path = url.replace(BASE, "")
    if cr:
        r = cr.get(url, cookies=cookies, headers=HEADERS, impersonate="chrome", timeout=15)
        if r.status_code != 200:
            log(f"curl_cffi GET {path} -> {r.status_code}: {r.text[:300]!r}")
            raise HttpErr(r.status_code, r.text[:2000])
        return r.json()
    hdr = dict(HEADERS)
    hdr["User-Agent"] = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
                         "(KHTML, like Gecko) Chrome/130.0 Safari/537.36")
    hdr["Cookie"] = "; ".join(f"{k}={v}" for k, v in cookies.items())
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=hdr), timeout=15) as r:
            return json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")[:2000]
        log(f"urllib GET {path} -> {e.code}: {body[:300]!r}")
        raise HttpErr(e.code, body)


def fetch_usage(cookies):
    org = cookies.get("lastActiveOrg")   # организация, открытая сейчас в приложении
    if not org:
        orgs = http_get(f"{BASE}/organizations", cookies)
        chat = [o for o in orgs if "chat" in (o.get("capabilities") or [])]
        org = (chat or orgs)[0]["uuid"]
    return http_get(f"{BASE}/organizations/{org}/usage", cookies)


# ---------------- helpers ----------------
def until(iso):
    if not iso:
        return ""
    try:
        t = datetime.fromisoformat(iso.replace("Z", "+00:00"))
    except ValueError:
        return ""
    sec = int((t - datetime.now(timezone.utc)).total_seconds())
    if sec <= 0:
        return "сейчас"
    m = sec // 60
    h, m = divmod(m, 60)
    d, h = divmod(h, 24)
    if d:
        return f"{d}д {h}ч"
    if h:
        return f"{h}ч {m:02d}м"
    return f"{m}м"


def bar_color(used):
    if used < 50:
        return "#4cc38a"   # зелёный
    if used < 80:
        return "#f5c542"   # жёлтый
    return "#e5484d"       # красный


def describe(e):
    if isinstance(e, HttpErr):
        if e.cloudflare:
            return f"Cloudflare блокирует запрос ({e.code}) · см. log.txt"
        if e.code in (401, 403):
            return f"сессия не принята ({e.code}) · ПКМ → sessionKey"
        if e.code == 429:
            return "слишком часто, подожду"
        return f"ошибка claude.ai ({e.code})"
    if isinstance(e, (urllib.error.URLError, TimeoutError)):
        return "нет сети"
    return str(e)[:60] or e.__class__.__name__


# ---------------- UI ----------------
class App:
    def __init__(self):
        self.cfg = load_cfg()
        self.q = queue.Queue()
        self.data, self.err, self.updated, self.busy = None, None, None, False
        self.backoff = 0
        self.timer = None

        r = self.root = tk.Tk()
        r.title("Claude Limits")
        r.overrideredirect(True)
        r.configure(bg=KEY)
        r.attributes("-topmost", self.cfg.get("topmost", True))
        try:
            r.attributes("-transparentcolor", KEY)
        except tk.TclError:
            pass
        x, y = self.cfg.get("pos", [r.winfo_screenwidth() - 300, 40])
        r.geometry(f"+{x}+{y}")

        self.cv = tk.Canvas(r, width=W, height=120, bg=KEY, highlightthickness=0, bd=0)
        self.cv.pack()
        self.cat_t = 0

        self.topmost = tk.BooleanVar(value=self.cfg.get("topmost", True))
        self.menu = tk.Menu(r, tearoff=0)
        self.menu.add_command(label="Обновить", command=self.refresh)
        self.interval = tk.IntVar(value=self.cfg.get("interval", DEFAULT_INTERVAL))
        sub = tk.Menu(self.menu, tearoff=0)
        for sec in INTERVALS:
            sub.add_radiobutton(label=f"каждые {sec} с", value=sec, variable=self.interval,
                                command=self.set_interval)
        self.menu.add_cascade(label="Автообновление", menu=sub)
        self.menu.add_checkbutton(label="Поверх окон", variable=self.topmost, command=self.toggle_top)
        self.menu.add_command(label="Ввести sessionKey…", command=self.ask_key)
        self.menu.add_command(label="Сбросить sessionKey", command=self.clear_key)
        self.menu.add_command(label="Открыть лог", command=lambda: os.startfile(LOG_PATH)
                              if os.path.exists(LOG_PATH) else None)
        self.menu.add_separator()
        self.menu.add_command(label="Выход", command=r.destroy)

        r.bind_all("<ButtonPress-1>", self._press)
        r.bind_all("<B1-Motion>", self._drag)
        r.bind_all("<ButtonRelease-1>", self._release)
        r.bind_all("<Double-Button-1>", lambda e: self.refresh())
        r.bind_all("<Button-3>", lambda e: self.menu.tk_popup(e.x_root, e.y_root))

        self.render()
        self.refresh()
        r.after(120, self.animate)
        r.after(200, self.poll)
        r.after(TICK_MS, self.tick)

    # --- окно ---
    def _press(self, e):
        self._dx, self._dy = e.x_root - self.root.winfo_x(), e.y_root - self.root.winfo_y()

    def _drag(self, e):
        self.root.geometry(f"+{e.x_root - self._dx}+{e.y_root - self._dy}")

    def _release(self, e):
        self.cfg["pos"] = [self.root.winfo_x(), self.root.winfo_y()]
        save_cfg(self.cfg)

    def toggle_top(self):
        self.cfg["topmost"] = self.topmost.get()
        self.root.attributes("-topmost", self.cfg["topmost"])
        save_cfg(self.cfg)

    def set_interval(self):
        self.cfg["interval"] = self.interval.get()
        save_cfg(self.cfg)
        self.refresh()

    def schedule(self):
        if self.timer:
            self.root.after_cancel(self.timer)
        sec = self.backoff or self.interval.get()
        self.timer = self.root.after(sec * 1000, self.refresh)

    def ask_key(self):
        # Ctrl+V/C/X/A при русской раскладке (Tk смотрит на букву, а не на клавишу)
        def ru_keys(e):
            ev = {86: "<<Paste>>", 67: "<<Copy>>", 88: "<<Cut>>"}.get(e.keycode)
            if ev and e.keysym.lower() not in ("v", "c", "x"):
                e.widget.event_generate(ev)
                return "break"
        self.root.bind_class("Entry", "<Control-KeyPress>", ru_keys, add="+")
        key = simpledialog.askstring(
            "sessionKey",
            "claude.ai → F12 → Application → Cookies → sessionKey\n(хранится зашифрованно через DPAPI)",
            parent=self.root)
        if key and key.strip():
            self.cfg["sk"] = base64.b64encode(dpapi(key.strip().encode(), protect=True)).decode()
            save_cfg(self.cfg)
            self.refresh()

    def clear_key(self):
        self.cfg.pop("sk", None)
        save_cfg(self.cfg)
        self.refresh()

    # --- данные ---
    def cookie_sources(self):
        for d in claude_dirs():
            try:
                yield read_desktop_cookies(d)
            except Exception as e:
                self._last_src_err = e
        if self.cfg.get("sk"):
            yield {"sessionKey": dpapi(base64.b64decode(self.cfg["sk"])).decode()}

    def refresh(self):
        if self.busy:
            return
        if self.timer:
            self.root.after_cancel(self.timer)
            self.timer = None
        self.busy = True
        threading.Thread(target=self._work, daemon=True).start()

    def _work(self):
        self._last_src_err = None
        last = None
        try:
            for cookies in self.cookie_sources():
                try:
                    self.q.put(("ok", fetch_usage(cookies)))
                    return
                except Exception as e:
                    last = e
            last = last or self._last_src_err or RuntimeError("Claude Desktop не найден · ПКМ → sessionKey")
            self.q.put(("err", last))
        except Exception as e:
            self.q.put(("err", e))

    def poll(self):
        try:
            kind, val = self.q.get_nowait()
        except queue.Empty:
            pass
        else:
            self.busy = False
            if kind == "ok":
                self.data, self.err, self.updated = val, None, datetime.now()
                self.backoff = 0
            else:
                self.err = describe(val)
                if isinstance(val, HttpErr) and val.code == 429:
                    self.backoff = min(MAX_BACKOFF, max(self.interval.get(), self.backoff) * 2)
            self.render()
            self.schedule()
        self.root.after(200, self.poll)

    def tick(self):
        self.render()
        self.root.after(TICK_MS, self.tick)

    # --- отрисовка ---
    def animate(self):
        self.cat_t += 1
        self.draw_cat()
        self.root.after(125, self.animate)

    def draw_cat(self):
        cv = self.cv
        cv.delete("cat")
        ox = W - 10 * CAT_PX - 14             # справа в шапке
        oy = TOP + HEADER_H - 8 * CAT_PX      # сидит на разделителе
        for (x, y), c in cat_frame(self.cat_t).items():
            x0, y0 = ox + x * CAT_PX, oy + y * CAT_PX
            cv.create_rectangle(x0, y0, x0 + CAT_PX, y0 + CAT_PX,
                                fill=CAT_COLORS[c], width=0, tags="cat")

    def render(self):
        cv = self.cv
        cv.delete("ui")
        rows = []
        if self.data:
            for key, name in ROWS:
                win = self.data.get(key)
                if not win and key not in ("five_hour", "seven_day"):
                    continue
                rows.append((name, win or {}))
        msg = self.err or (None if self.data else "загрузка…")
        h = TOP + HEADER_H + len(rows) * ROW_H + (18 if msg else 0) + 8
        cv.config(height=h + 1)

        rounded_rect(cv, 0, TOP, W - 1, h, 12, fill=BG, outline=BORDER, tags="ui")
        # шапка
        sparkle(cv, 17, TOP + 14, 6, ACCENT, tags="ui")
        cv.create_text(29, TOP + 14, text="Claude", fill=FG, font=FONT_TITLE, anchor="w", tags="ui")
        dot = "#e5484d" if self.err else ("#5bb98b" if self.data else DIM)
        cv.create_oval(80, TOP + 12, 84, TOP + 16, fill=dot, outline="", tags="ui")
        if self.updated:
            cv.create_text(89, TOP + 14, text=self.updated.strftime("%H:%M"), fill=DIM,
                           font=FONT_SMALL, anchor="w", tags="ui")
        cv.create_line(12, TOP + HEADER_H, W - 12, TOP + HEADER_H, fill=BORDER, tags="ui")

        y = TOP + HEADER_H + ROW_H // 2 + 3
        bx0, bx1 = 66, 160
        for name, win in rows:
            used = min(100.0, max(0.0, float(win.get("utilization") or 0)))
            cv.create_text(12, y, text=name, fill=FG, font=FONT, anchor="w", tags="ui")
            cv.create_line(bx0, y, bx1, y, fill=TRACK, width=6, capstyle="round", tags="ui")
            if used > 0:
                fx = bx0 + (bx1 - bx0) * used / 100
                cv.create_line(bx0, y, max(fx, bx0 + 0.1), y, fill=bar_color(used), width=6,
                               capstyle="round", tags="ui")
            cv.create_text(200, y, text=f"{used:.0f}%", fill=FG, font=FONT, anchor="e", tags="ui")
            cv.create_text(W - 12, y, text=until(win.get("resets_at")), fill=DIM,
                           font=FONT_SMALL, anchor="e", tags="ui")
            y += ROW_H
        if msg:
            cv.create_text(12, y - 2, text=msg, fill="#e8a23a" if self.err else DIM, font=FONT_SMALL,
                           anchor="w", width=W - 24, tags="ui")
        self.draw_cat()

    def run(self):
        self.root.mainloop()


if __name__ == "__main__":
    if sys.platform != "win32":
        sys.exit("Только для Windows")
    App().run()
