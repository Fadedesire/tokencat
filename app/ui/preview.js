// Превью S06: все компоненты во всех состояниях на выдуманных данных. Инструмент разработки:
// подписи превью по-русски, строки компонентов — из словаря. Словарь (B3) грузится из
// ../locales/<lang>.json, поэтому превью открывать через локальный сервер из папки app
// (например `npx serve app` → /ui/preview.html?lang=de); без сервера будут видны ключи.
(async function () {
  "use strict";
  const lang = new URLSearchParams(location.search).get("lang") || "ru";
  try {
    const r = await fetch("../locales/" + lang + ".json");
    TokenCat.i18n.load(await r.json(), lang);
  } catch (_) {}
  const { Panel } = TokenCat.ui;
  const $ = (id) => document.getElementById(id);
  const now = Date.now();
  const at = (min) => new Date(now + min * 60000).toISOString();
  const lim = (kind, percent, inMin) => ({ kind, percent, resets_at: inMin == null ? null : at(inMin) });

  // ---- Цвета ----
  const css = getComputedStyle(document.documentElement);
  for (const [name, v] of [
    ["Фон", "--tc-bg"], ["Текст", "--tc-text"], ["Вторичный", "--tc-muted"], ["Акцент", "--tc-accent"],
    ["0–49 %", "--tc-green"], ["50–79 %", "--tc-yellow"], ["80–100 %", "--tc-red"],
    ["Дорожка", "--tc-track"], ["Рамка", "--tc-border"], ["Предупр.", "--tc-warn"],
  ]) {
    const d = document.createElement("div");
    d.className = "sw";
    const i = document.createElement("i");
    const val = css.getPropertyValue(v).trim();
    i.style.background = val;
    const b = document.createElement("b");
    b.textContent = name;
    d.append(i, b, document.createTextNode(val));
    $("swatches").appendChild(d);
  }

  // ---- Строки лимита на порогах ----
  const rowSnap = (list) => ({ state: "ok", usage: { session: null, weekly: null, others: list },
    updated_at_ms: now });
  const thresholds = [
    lim("session", 0, null), lim("session", 12, 263), lim("session", 49, 95),
    lim("session", 50, 41), lim("session", 79, 12), lim("session", 80, 7), lim("session", 100, 0.5),
  ];
  const rp = new Panel({ cat: false, title: "Пороги" });
  rp.update(rowSnap(thresholds));
  $("rows").appendChild(rp.root);

  // ---- Панели ----
  const base = {
    session: lim("session", 24, 262),
    weekly: lim("weekly_all", 63, 4 * 1440 + 11 * 60),
    others: [],
    breakdown: [{ name: "Чаты", percent: 5 }, { name: "Cowork", percent: 95 }],
  };
  const full = { ...base, others: [lim("weekly_sonnet", 38, 4 * 1440 + 11 * 60), lim("weekly_opus", 0, null)] };
  const hot = { ...base, session: lim("session", 86, 38), weekly: lim("weekly_all", 97, 1440 + 180) };
  const fresh = { ...base, session: lim("session", 0, null), weekly: lim("weekly_all", 18, 6 * 1440) };
  const ago = (s) => now - s * 1000;

  const cases = [
    ["Всё работает", "5 ч и неделя", { state: "ok", message: "Всё работает", usage: base, updated_at_ms: ago(12) }],
    ["Всё работает", "все окна + доля недели", { state: "ok", usage: full, updated_at_ms: ago(40) }, { breakdown: true }],
    ["80 %+", "котик встревожен", { state: "ok", usage: hot, updated_at_ms: ago(5) }],
    ["Окно 5 ч не начато", "0 % без времени сброса", { state: "ok", usage: fresh, updated_at_ms: ago(3) }],
    ["Обновляю", "идёт запрос", { state: "ok", usage: base, updated_at_ms: ago(30), fetching: true }],
    ["Загрузка", "первый запуск, данных ещё нет", { state: "loading", message: "Загружаю данные…" }],
    ["Не вошли", "котик спит", { state: "signed_out", message: "Вы не вошли в claude.ai" }],
    ["Сессия истекла", "старые цифры приглушены", { state: "expired", message: "Нужно войти заново", usage: base, updated_at_ms: ago(3600) }],
    ["Регион / VPN", "данные есть, устарели", { state: "blocked", message: "claude.ai недоступен из вашей сети. Проверьте VPN", usage: base, updated_at_ms: ago(300) }],
    ["Регион / VPN", "данных нет", { state: "blocked", message: "claude.ai недоступен из вашей сети. Проверьте VPN" }],
    ["Нет сети", "", { state: "offline", message: "Нет соединения с claude.ai", usage: base, updated_at_ms: ago(120) }],
    ["Прокси", "", { state: "offline", message: "Прокси не отвечает. Проверьте адрес прокси в настройках" }],
    ["Ждём (429)", "", { state: "error", message: "claude.ai просит подождать, повторю позже", usage: base, updated_at_ms: ago(90) }],
    ["Формат изменился", "ссылка на лог", { state: "error", message: "Не удалось прочитать данные", show_log: true }],
  ];
  const log = (id) => console.log("[preview] action:", id);
  for (const [title, note, snap, opts] of cases) {
    const cell = document.createElement("div");
    cell.className = "cell";
    const cap = document.createElement("div");
    cap.className = "cap";
    const b = document.createElement("b");
    b.textContent = title;
    cap.append(b, document.createTextNode(note ? " — " + note : ""));
    const p = new Panel({ onAction: log, ...(opts || {}) });
    p.update(snap);
    cell.append(cap, p.root);
    $("panels").appendChild(cell);
    setInterval(() => p.tick(), 1000);
  }

  // ---- Котик: настроения и размеры ----
  for (const [mood, name] of [["idle", "обычный"], ["sleep", "спит (нет данных)"], ["alert", "встревожен (80 %+)"]]) {
    for (const px of [2, 4]) {
      const box = document.createElement("div");
      box.className = "catbox";
      const stage = document.createElement("div");
      stage.className = "stage";
      const cv = document.createElement("canvas");
      stage.appendChild(cv);
      box.append(stage, document.createTextNode(`${name} · ×${px}`));
      $("cats").appendChild(box);
      new TokenCat.Cat(cv, { px, mood });
    }
  }
})();
