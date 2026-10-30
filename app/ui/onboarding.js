// Онбординг первого запуска (B1, п. 7). Шаги: привет и ОС → вид → вход → ранний старт (B9) →
// автозапуск.
// Добавленный аккаунт (не основной профиль) проходит только вход и автозапуск.
// «Готово» (или крестик окна) — онбординг пройден, больше не показывается;
// повторить можно в настройках («Пройти заново»).
TokenCat.i18n.ready.then(function () {
  "use strict";
  const T = TokenCat.T;
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);

  let st = null;          // настройки (SettingsView)
  let auth = { signed_in: false };
  let steps = [];
  let at = 0;
  let view = "overlay";

  // Пример данных для превью панели
  const DEMO = {
    state: "ok",
    updated_at_ms: Date.now(),
    usage: {
      session: { kind: "session", percent: 42, resets_at: new Date(Date.now() + 3 * 3600e3 + 12 * 60e3).toISOString() },
      weekly: { kind: "weekly_all", percent: 18, resets_at: new Date(Date.now() + 4 * 86400e3).toISOString() },
      others: [],
    },
  };

  function renderSteps() {
    const box = $("steps");
    box.innerHTML = "";
    steps.forEach((_, i) => {
      const d = document.createElement("i");
      if (i <= at) d.className = "on";
      box.appendChild(d);
    });
    for (const sec of document.querySelectorAll(".step")) {
      sec.hidden = sec.dataset.step !== steps[at];
    }
    const name = steps[at];
    $("back").hidden = at === 0;
    $("skip").hidden = !(name === "login" && !auth.signed_in);
    $("next").textContent = T(at === steps.length - 1 ? "onb.finish" : name === "hello" ? "onb.start" : "onb.next");
    $("next").disabled = name === "login" && !auth.signed_in;
    if (name === "login") renderLogin();
  }

  function go(delta) {
    at = Math.max(0, Math.min(steps.length - 1, at + delta));
    renderSteps();
  }

  // ---------- 1. Привет ----------
  // Названия языков — на самих языках, как в настройках
  const LANG_NAMES = {
    ru: "Русский", en: "English", es: "Español", "pt-BR": "Português (Brasil)", de: "Deutsch",
    fr: "Français", ja: "日本語", ko: "한국어", "zh-Hans": "简体中文",
  };

  function initHello() {
    $("os-name").textContent = st.os === "macos" ? "macOS" : "Windows";
    // Язык (B3): по умолчанию — как в системе. Выбор сохраняем сразу; Rust пришлёт
    // lang-changed, и окно перезагрузится уже на новом языке (на этом же шаге).
    $("lang-en").hidden = TokenCat.i18n.lang === "en";
    const sel = $("language");
    for (const l of st.languages) {
      const o = document.createElement("option");
      o.value = l;
      o.textContent = l === "system" ? T("lang.system") : LANG_NAMES[l] || l;
      if (l === st.language) o.selected = true;
      sel.appendChild(o);
    }
    sel.onchange = () => invoke("settings_set", { patch: { language: sel.value } }).catch(() => {});
    if (TokenCat.Cat) new TokenCat.Cat($("hello-cat"), { px: 5, mood: "idle" });
  }

  // ---------- 2. Вид ----------
  function previewOverlay() {
    const p = document.createElement("div");
    p.className = "preview";
    const mini = document.createElement("div");
    mini.className = "mini";
    const panel = new TokenCat.ui.Panel({});
    panel.update(DEMO);
    mini.appendChild(panel.root);
    p.appendChild(mini);
    setInterval(() => panel.tick(), 1000);
    return p;
  }

  function previewSecond() {
    const p = document.createElement("div");
    p.className = "preview";
    if (st.os === "macos") {
      const bar = document.createElement("div");
      bar.className = "menubar";
      bar.textContent = `🐱 ${T("short.session")} 42% · ${T("short.weekly")} 18%`;
      p.appendChild(bar);
    } else {
      const bar = document.createElement("div");
      bar.className = "bar";
      bar.innerHTML = '<span class="start"></span><span class="app"></span><span class="app"></span>';
      const w = document.createElement("div");
      w.className = "widget";
      // Котик и на панели задач (по умолчанию включён, как на плавающей панели)
      if (TokenCat.Cat) {
        const cv = document.createElement("canvas");
        cv.className = "bar-cat";
        bar.appendChild(cv);
        new TokenCat.Cat(cv, { px: 1, mood: "idle" });
      }
      for (const [label, pct] of [[T("short.session"), 42], [T("short.weekly"), 18]]) {
        const row = document.createElement("span");
        const b = document.createElement("b");
        b.style.setProperty("--w", pct + "%");
        row.append(label, b, pct + "%");
        w.appendChild(row);
      }
      bar.appendChild(w);
      p.appendChild(bar);
    }
    return p;
  }

  function initCards() {
    const second = st.os === "macos"
      ? { id: "menubar", name: T("view.menubar"), desc: T("onb.card_menubar") }
      : { id: "taskbar", name: T("view.taskbar"), desc: T("onb.card_taskbar") };
    const cards = [
      { id: "overlay", name: T("view.overlay"), desc: T("onb.card_overlay"), preview: previewOverlay },
      { ...second, preview: previewSecond },
    ];
    const box = $("cards");
    for (const c of cards) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "card";
      const ready = st.views.includes(c.id);
      b.disabled = !ready;
      b.setAttribute("aria-pressed", String(view === c.id));
      b.appendChild(c.preview());
      const n = document.createElement("div");
      n.className = "name";
      n.textContent = c.name + " ";
      if (!ready) {
        const s = document.createElement("span");
        s.className = "soon";
        s.textContent = "— " + T("onb.soon");
        n.appendChild(s);
      }
      const d = document.createElement("div");
      d.className = "desc";
      d.textContent = c.desc;
      b.append(n, d);
      b.onclick = () => {
        view = c.id;
        for (const x of box.children) x.setAttribute("aria-pressed", String(x === b));
      };
      box.appendChild(b);
    }
  }

  // ---------- 3. Вход ----------
  function renderLogin() {
    const msg = $("login-msg");
    if (auth.signed_in) {
      $("login").hidden = true;
      msg.hidden = false;
      msg.className = "msg ok";
      msg.textContent = st.account_name ? T("onb.logged_as", { name: st.account_name }) : T("onb.logged");
    } else {
      $("login").hidden = false;
    }
  }

  $("login").onclick = async () => {
    const msg = $("login-msg");
    msg.hidden = false;
    msg.className = "msg muted";
    msg.textContent = T("login.wait");
    try { await invoke("auth_login"); }
    catch (e) { msg.className = "msg bad"; msg.textContent = String(e); }
  };

  async function loadAuth() {
    try { auth = await invoke("auth_status"); } catch (_) { auth = { signed_in: false }; }
  }

  listen("auth-changed", async (ev) => {
    const p = ev.payload || {};
    const msg = $("login-msg");
    if (p.kind === "problem" || (p.kind === "cancelled" && p.message)) {
      msg.hidden = false;
      msg.className = "msg warn";
      msg.textContent = p.message;
      return;
    }
    if (p.kind === "cancelled") {
      msg.hidden = false;
      msg.className = "msg muted";
      msg.textContent = T("onb.login_closed");
      return;
    }
    await loadAuth();
    renderSteps();
    // Сразу после входа — следующий шаг
    if (auth.signed_in && steps[at] === "login") setTimeout(() => go(1), 900);
  });
  listen("settings-changed", (ev) => {
    st = ev.payload;
    if (steps[at] === "login") renderLogin();
  });

  // ---------- 4. Ранний старт (B9) ----------
  // Выбор хранится в early.st и сохраняется по «Готово» вместе с остальным
  const early = new TokenCat.ui.EarlyStart({ onChange: () => renderTimeline() });
  $("early-box").appendChild(early.root);

  /** Таймлайн: запрос задачи → сели работать (через 3 ч) → сброс. */
  function renderTimeline() {
    const E = TokenCat.early;
    const h = early.st.early_reset_hour;
    const on = early.st.early_start;
    const t = E.taskTime(h);
    const work = (h + 22) % 24;               // за 2 ч до сброса
    const box = $("timeline");
    box.innerHTML = "";
    box.classList.toggle("off", !on);
    const pts = [
      { at: 0, time: E.hhmm(t.h, t.m), text: T("onb.tl_task") },
      { at: 60, time: E.hhmm(work, 0), text: T("onb.tl_work") },
      { at: 100, time: E.hhmm(h, 0), text: T("onb.tl_reset") },
    ];
    const line = document.createElement("div");
    line.className = "tl-line";
    box.appendChild(line);
    for (const p of pts) {
      const d = document.createElement("div");
      d.className = "tl-pt";
      d.style.left = p.at + "%";
      const b = document.createElement("b");
      b.textContent = p.time;
      const s = document.createElement("span");
      s.textContent = p.text;
      d.append(b, s);
      box.appendChild(d);
    }
  }

  // ---------- 5. Готово ----------
  async function finish() {
    $("next").disabled = true;
    try {
      if (view !== st.view) await invoke("settings_set", { patch: { view } });
      // Ранний старт: по умолчанию вкл (решение Ромы 30.10) — сохраняем как выбрано на шаге
      await invoke("settings_set", { patch: { early_start: early.st.early_start, early_reset_hour: early.st.early_reset_hour } });
      await invoke("onboarding_finish", { autostart: $("autostart").checked });
    } catch (e) {
      // Сюда попадаем, только если не сохранились настройки (окно тогда не закрылось)
      const m = $("finish-msg");
      m.hidden = false;
      m.textContent = String(e);
      $("next").disabled = false;
    }
  }

  $("next").onclick = () => (at === steps.length - 1 ? finish() : go(1));
  $("back").onclick = () => go(-1);
  $("skip").onclick = () => go(1);

  (async () => {
    try { st = await invoke("settings_get"); } catch (_) { return; }
    document.title = "TokenCat — " + T("win.welcome");
    await loadAuth();
    view = st.view;
    // Автозапуск в онбординге всегда предлагаем включённым (решение Ромы 29.10)
    $("autostart").checked = true;
    early.set(st);
    renderTimeline();
    if (st.profile_default) {
      steps = ["hello", "view", "login", "early", "autostart"];
      initHello();
      initCards();
    } else {
      // Второй аккаунт: ОС и вид уже выбраны в основном
      steps = ["login", "early", "autostart"];
      $("login-title").textContent = T("onb.new_acc");
      $("login-lead").textContent = T("onb.new_acc_lead");
    }
    renderSteps();
  })();
});
