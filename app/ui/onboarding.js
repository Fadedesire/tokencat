// Онбординг первого запуска (B1, п. 7). Шаги: привет и ОС → вид → вход → автозапуск.
// Добавленный аккаунт (не основной профиль) проходит только вход и автозапуск.
// «Готово» (или крестик окна) — онбординг пройден, больше не показывается;
// повторить можно в настройках («Пройти заново»).
(function () {
  "use strict";
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
    $("next").textContent = at === steps.length - 1 ? "Готово" : name === "hello" ? "Начать" : "Дальше";
    $("next").disabled = name === "login" && !auth.signed_in;
    if (name === "login") renderLogin();
  }

  function go(delta) {
    at = Math.max(0, Math.min(steps.length - 1, at + delta));
    renderSteps();
  }

  // ---------- 1. Привет ----------
  function initHello() {
    $("os-name").textContent = st.os === "macos" ? "macOS" : "Windows";
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
      bar.textContent = "🐱 5ч 42% · Н 18%";
      p.appendChild(bar);
    } else {
      const bar = document.createElement("div");
      bar.className = "bar";
      bar.innerHTML = '<span class="start"></span><span class="app"></span><span class="app"></span>';
      const w = document.createElement("div");
      w.className = "widget";
      for (const [label, pct] of [["5ч", 42], ["Н", 18]]) {
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
      ? { id: "menubar", name: "В строке меню", desc: "Котик и проценты рядом с часами. Клик — подробная панель." }
      : { id: "taskbar", name: "В панели задач", desc: "Две мини-полоски у трея. Клик — подробная панель." };
    const cards = [
      { id: "overlay", name: "Поверх окон", desc: "Маленькая панель на рабочем столе. Можно двигать, всегда на виду.", preview: previewOverlay },
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
        s.textContent = "— скоро";
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
      msg.textContent = st.account_name ? `Готово! Вы вошли как ${st.account_name}.` : "Готово! Вы вошли в claude.ai.";
    } else {
      $("login").hidden = false;
    }
  }

  $("login").onclick = async () => {
    const msg = $("login-msg");
    msg.hidden = false;
    msg.className = "msg muted";
    msg.textContent = "Войдите в открывшемся окне claude.ai…";
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
      msg.textContent = "Окно входа закрыто. Можно попробовать ещё раз или пропустить.";
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

  // ---------- 4. Готово ----------
  async function finish() {
    $("next").disabled = true;
    try {
      if (view !== st.view) await invoke("settings_set", { patch: { view } });
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
    await loadAuth();
    view = st.view;
    $("autostart").checked = st.onboarded ? st.autostart : true;
    if (st.profile_default) {
      steps = ["hello", "view", "login", "autostart"];
      initHello();
      initCards();
    } else {
      // Второй аккаунт: ОС и вид уже выбраны в основном
      steps = ["login", "autostart"];
      $("login-title").textContent = "Новый аккаунт";
      $("login-lead").textContent = "Войдите в другой аккаунт claude.ai — у этой панели будут свои лимиты. " +
        "Окно входа откроется «с чистого листа», первый аккаунт не пострадает.";
    }
    renderSteps();
  })();
})();
