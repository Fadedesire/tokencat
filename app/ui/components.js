// TokenCat — общие компоненты интерфейса (S06).
// Panel — панель как в прототипе: шапка (название, точка статуса, время, котик), строки лимитов,
// сообщение о состоянии с кнопкой. Ничего не знает о Tauri: получает снимок планировщика
// (Snapshot из scheduler.rs) и сообщает о нажатиях через onAction("login" | "log" | "refresh").
// Таймеры «до сброса» тикают локально раз в секунду, без запросов.
(function () {
  "use strict";
  const TC = (window.TokenCat = window.TokenCat || {});

  // Подписи лимитов. Незнакомые kind показываем как есть (S12 переведёт).
  const LABELS = {
    session: "5 ч",
    weekly_all: "Неделя",
    weekly_sonnet: "Sonnet",
    weekly_opus: "Opus",
  };

  /** Цвет полоски по порогам SPEC: 0–49 зелёный, 50–79 жёлтый, 80–100 красный. */
  function level(p) { return p >= 80 ? "red" : p >= 50 ? "yellow" : "green"; }

  /** Время до сброса в стиле прототипа: «4ч 05м», «3д 4ч», «45м», «сейчас». */
  function fmtUntil(iso, now = Date.now()) {
    if (!iso) return "";
    const t = Date.parse(iso);
    if (isNaN(t)) return "";
    const sec = Math.floor((t - now) / 1000);
    if (sec <= 0) return "сейчас";
    let m = Math.floor(sec / 60);
    const h = Math.floor(m / 60) % 24, d = Math.floor(m / 1440);
    m %= 60;
    if (d) return `${d}д ${h}ч`;
    if (h) return `${h}ч ${String(m).padStart(2, "0")}м`;
    return m ? `${m}м` : "<1м";
  }

  function fmtClock(ms) {
    const d = new Date(ms);
    return String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
  }

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  /** Строки для показа: 5 ч и неделя всегда первыми, потом остальные окна. */
  function limitsOf(usage) {
    if (!usage) return [];
    return [usage.session, usage.weekly, ...(usage.others || [])].filter(Boolean);
  }

  /**
   * Одна строка лимита. Отдельно — чтобы её можно было вставить и в другие виды
   * (всплывающая панель в трее, поповер строки меню).
   */
  class LimitRow {
    constructor() {
      this.root = el("div", "tc-row");
      this.label = el("span", "tc-label");
      const bar = el("span", "tc-bar");
      this.fill = el("span", "tc-fill");
      bar.appendChild(this.fill);
      this.pct = el("span", "tc-pct");
      this.until = el("span", "tc-until");
      this.root.append(this.label, bar, this.pct, this.until);
      this.iso = null;
      this.notStarted = false;
    }
    set(limit) {
      const p = Math.min(100, Math.max(0, Number(limit.percent) || 0));
      this.label.textContent = LABELS[limit.kind] || limit.label || limit.kind;
      this.root.title = this.label.textContent;
      this.fill.style.width = p + "%";
      this.fill.className = "tc-fill " + level(p);
      this.pct.textContent = Math.round(p) + "%";
      this.iso = limit.resets_at || null;
      // У 5-часового окна при 0 % может не быть resets_at: окно ещё не начато
      this.notStarted = !this.iso && p === 0;
      this.tick();
    }
    tick(now) {
      this.until.textContent = this.notStarted ? "не начато" : fmtUntil(this.iso, now);
    }
  }

  /**
   * Состояние панели из снимка планировщика:
   *   dot  — ok | warn | bad | idle
   *   kind — тон сообщения: "" | warn | bad
   *   mood — настроение котика: idle | sleep | alert
   */
  function describe(snap) {
    const s = snap || { state: "loading", message: "Загружаю данные…" };
    const rows = limitsOf(s.usage);
    const has = rows.length > 0;
    const max = rows.reduce((a, l) => Math.max(a, Number(l.percent) || 0), 0);
    const r = { rows, stale: has && s.state !== "ok", message: "", kind: "", action: null,
                dot: "idle", mood: has ? (max >= 80 ? "alert" : "idle") : "sleep" };
    switch (s.state) {
      case "ok":
        r.dot = "ok";
        if (!has) r.message = "Нет данных о лимитах";
        break;
      case "loading":
        r.message = s.message || "Загружаю данные…";
        break;
      case "signed_out":
        r.message = s.message || "Вы не вошли в claude.ai";
        r.action = { id: "login", text: "Войти", primary: true };
        r.mood = "sleep";
        r.rows = []; r.stale = false;
        break;
      case "expired":
        r.dot = "bad"; r.kind = "warn";
        r.message = s.message || "Нужно войти заново";
        r.action = { id: "login", text: "Войти заново", primary: true };
        break;
      case "blocked":
      case "offline":
        r.dot = has ? "warn" : "bad"; r.kind = "warn";
        r.message = s.message;
        break;
      default:            // error: 429, 5xx, неожиданный ответ
        r.dot = has ? "warn" : "bad"; r.kind = "warn";
        r.message = s.message || "Не удалось прочитать данные";
        break;
    }
    if (s.show_log && !r.action) r.action = { id: "log", text: "Открыть лог", link: true };
    if (s.fetching && !has && s.state !== "signed_out") r.message = "Обновляю…";
    return r;
  }

  class Panel {
    /**
     * opts.onAction(id) — нажатия кнопок в сообщении; opts.cat=false — без котика;
     * opts.breakdown=true — строка «доля недели по продуктам».
     */
    constructor(opts = {}) {
      this.opts = opts;
      this.root = el("section", "tc-panel");
      const head = (this.head = el("header", "tc-head"));
      this.title = el("span", "tc-title", opts.title || "TokenCat");
      this.dot = el("i", "tc-dot");
      this.time = el("span", "tc-time");
      head.append(this.title, this.dot, this.time);
      this.cat = null;
      this.setCat(opts.cat !== false);
      this.rowsBox = el("div", "tc-rows");
      this.note = el("div", "tc-note"); this.note.hidden = true;
      this.msg = el("div", "tc-msg"); this.msg.hidden = true;
      this.msgText = el("span", "tc-msg-text");
      this.msgBtn = el("button", "tc-btn"); this.msgBtn.type = "button";
      this.msg.append(this.msgText, this.msgBtn);
      this.msgBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        const id = this.msgBtn.dataset.action;
        if (id && this.opts.onAction) this.opts.onAction(id);
      });
      // Облачко котика с фактом дня (B9): под шапкой, хвостик тянется к котику
      this.fact = el("div", "tc-fact"); this.fact.hidden = true;
      this.factText = el("span", "tc-fact-text");
      this.fact.appendChild(this.factText);
      this.fact.title = "Нажмите, чтобы закрыть до завтра";
      // mousedown, а не click: иначе нажатие на панели начинает перетаскивание окна
      this.fact.addEventListener("mousedown", (e) => {
        if (e.button !== 0) return;
        e.stopPropagation();
        const cb = this.onFactClose;
        this.setFact(null);
        if (cb) cb();
      });
      this.root.append(head, this.fact, this.rowsBox, this.note, this.msg);
      this.rows = [];
      this.override = null;
    }

    /**
     * Облачко с фактом дня. text=null — спрятать. onClose — когда закрыли кликом.
     * Появляется плавно; без котика облачко не показываем (говорить некому).
     */
    setFact(text, onClose) {
      const on = !!text && !!this.cat;
      this.onFactClose = on ? onClose || null : null;
      if (!on) {
        this.fact.hidden = true;
        this.fact.classList.remove("show");
        return;
      }
      if (this.factText.textContent === text && !this.fact.hidden) return;
      this.factText.textContent = text;
      this.fact.hidden = false;
      this.fact.classList.remove("show");
      void this.fact.offsetWidth;          // перезапустить анимацию появления
      this.fact.classList.add("show");
    }

    /** Котик в шапке: включить/выключить без пересоздания панели (настройка «Котик»). */
    setCat(on) {
      if (on && !this.cat && TC.Cat) {
        const cv = el("canvas", "tc-cat");
        cv.setAttribute("aria-hidden", "true");
        this.head.appendChild(cv);
        this.cat = new TC.Cat(cv, { px: 2, animate: this.opts.animate !== false });
        if (this.view) this.cat.setMood(this.view.mood);
      } else if (!on && this.cat) {
        this.cat.destroy();
        this.cat.canvas.remove();
        this.cat = null;
        if (this.fact) this.setFact(null);
      }
      this.root.classList.toggle("tc-no-cat", !this.cat);
    }

    /** Имя аккаунта в шапке вместо «TokenCat» (длинное обрежется с «…»). */
    setTitle(name) {
      const t = name || this.opts.title || "TokenCat";
      if (this.title.textContent !== t) this.title.textContent = t;
      this.title.title = name ? "TokenCat · " + name : "TokenCat";
    }

    /** Временный текст вместо сообщения (например «Войдите в открывшемся окне…»). */
    flash(text, kind = "") {
      this.override = text ? { text, kind } : null;
      this._msg();
    }

    update(snap) {
      this.snap = snap;
      this.override = null;
      const d = (this.view = describe(snap));
      this.root.dataset.state = (snap && snap.state) || "loading";
      this.root.classList.toggle("tc-stale", d.stale);
      this.root.classList.toggle("tc-empty", d.rows.length === 0);
      this.dot.className = "tc-dot " + d.dot;
      this.dot.title = (snap && snap.message) || "";

      // Строки: переиспользуем DOM, чтобы полоски плавно ехали, а не мигали
      while (this.rows.length < d.rows.length) {
        const r = new LimitRow();
        this.rows.push(r);
        this.rowsBox.appendChild(r.root);
      }
      while (this.rows.length > d.rows.length) this.rows.pop().root.remove();
      d.rows.forEach((l, i) => this.rows[i].set(l));

      const parts = ((snap && snap.usage && snap.usage.breakdown) || []).filter((b) => b.percent > 0);
      this.note.hidden = !(this.opts.breakdown && parts.length && d.rows.length);
      if (!this.note.hidden) {
        this.note.textContent = "Неделя: " + parts.map((b) => `${b.name} ${Math.round(b.percent)}%`).join(" · ");
      }

      if (this.cat) this.cat.setMood(d.mood);
      this._msg();
      this.tick();
    }

    _msg() {
      const d = this.view || describe(null);
      const o = this.override;
      const text = o ? o.text : d.message;
      this.msg.hidden = !text;
      this.msg.className = "tc-msg" + ((o ? o.kind : d.kind) ? " " + (o ? o.kind : d.kind) : "");
      this.msgText.textContent = text || "";
      const a = o ? null : d.action;
      this.msgBtn.hidden = !a;
      if (a) {
        this.msgBtn.textContent = a.text;
        this.msgBtn.dataset.action = a.id;
        this.msgBtn.className = a.link ? "tc-link" : "tc-btn" + (a.primary ? " primary" : "");
      }
    }

    /** Раз в секунду: таймеры «до сброса» и время в шапке. */
    tick(now = Date.now()) {
      for (const r of this.rows) r.tick(now);
      const s = this.snap;
      this.time.textContent = s && s.fetching ? "…" : s && s.updated_at_ms ? fmtClock(s.updated_at_ms) : "";
    }

    destroy() { if (this.cat) this.cat.destroy(); this.root.remove(); }
  }

  TC.ui = { Panel, LimitRow, describe, level, fmtUntil, fmtClock, LABELS };
})();
