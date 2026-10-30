// Окно настроек (B1, по SPEC). Данные приходят от Rust: снимок планировщика (`usage-updated`),
// настройки (`settings-changed`), вход (`auth-changed`). Сам экран в сеть не ходит.
// Секрет в JS не попадает: Rust отдаёт только маску ключа.
// Всё применяется сразу: каждый переключатель отправляет одно поле в settings_set.
TokenCat.i18n.ready.then(function () {
  "use strict";
  const T = TokenCat.T;
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const show = (id, on) => { $(id).hidden = !on; };
  // «Language ·» перед словом «Язык» — чтобы найти пункт, даже если язык выбран по ошибке
  show("lang-en", TokenCat.i18n.lang !== "en");

  // Названия языков — на самих языках (так их ищут в списке), кроме «Как в системе»
  const LANG_NAMES = {
    ru: "Русский", en: "English", es: "Español",
    "pt-BR": "Português (Brasil)", de: "Deutsch", fr: "Français", ja: "日本語",
    ko: "한국어", "zh-Hans": "简体中文",
  };
  const langName = (l) => (l === "system" ? T("lang.system") : LANG_NAMES[l] || l);
  const VIEW_NAMES = { overlay: "view.overlay", taskbar: "view.taskbar", menubar: "view.menubar" };
  const MENUBAR_NAMES = { both: "mb.both", session: "mb.session", icon: "mb.icon", ring: "mb.ring" };

  let snap = null;   // последний снимок от планировщика
  let auth = { signed_in: false };
  let st = null;     // настройки (SettingsView из commands.rs)

  // Живая панель сверху — та же, что на рабочем столе, плюс доля недели по продуктам
  const panel = new TokenCat.ui.Panel({
    breakdown: true,
    onAction: (id) => {
      if (id === "login") login();
      else if (id === "log") invoke("open_log").catch((e) => panel.flash(String(e), "warn"));
    },
  });
  $("panel-slot").appendChild(panel.root);

  // Ранний старт (B9): переключатель, час сброса, проверка, инструкция
  const early = new TokenCat.ui.EarlyStart({ status: true, onChange: (patch) => set(patch) });
  $("early-box").appendChild(early.root);

  function note(id, text, cls) {
    const el = $(id);
    el.hidden = !text;
    el.className = (el.classList.contains("item") ? "msg item " : "msg ") + (cls || "muted");
    el.textContent = text || "";
  }

  // ---------- Аккаунт и панель ----------

  function viewSnap() {
    const s = snap || { state: "loading", message: T("panel.loading") };
    if (auth.error) return { ...s, state: "error", message: auth.error, usage: null };
    if (!auth.signed_in) return { state: "signed_out", message: T("panel.signed_out") };
    return s;
  }

  function render() {
    const s = viewSnap();
    const signedIn = auth.signed_in && s.state !== "signed_out";
    panel.update(s);
    const name = signedIn ? s.account_name || (st && st.account_name) : null;
    panel.setTitle(name);
    if (st) panel.setCat(st.show_cat);

    $("account-line").textContent = signedIn ? [name, auth.masked].filter(Boolean).join(" · ") : "";
    show("account-line", signedIn && (name || auth.masked));
    show("refresh", signedIn);
    $("refresh").disabled = !!s.fetching;
    show("login", !auth.signed_in);
    show("logout", auth.signed_in);
    $("acc-name").textContent = auth.signed_in ? name || T("acc.signed_in") : T("acc.signed_out");
    $("acc-hint").textContent = auth.signed_in ? T("acc.hint_in") : T("acc.hint_out");
  }

  async function loadAuth() {
    try { auth = await invoke("auth_status"); }
    catch (e) { auth = { signed_in: false, error: String(e) }; }
  }

  async function login() {
    panel.flash(T("login.wait"));
    try { await invoke("auth_login"); } catch (e) { panel.flash(String(e), "warn"); }
  }

  $("login").onclick = login;
  $("refresh").onclick = () => invoke("usage_refresh").catch(() => {});
  $("logout").onclick = async () => {
    try { await invoke("auth_logout"); } catch (e) { note("acc-msg", String(e), "bad"); }
  };
  $("add-account").onclick = async () => {
    $("add-account").disabled = true;
    try {
      await invoke("profile_add");
      note("acc-msg", T("acc.adding"), "ok");
    } catch (e) {
      note("acc-msg", String(e), "bad");
    }
    setTimeout(() => { $("add-account").disabled = false; }, 2000);
  };
  // Удаление профиля — с подтверждением прямо в окне (системные диалоги не используем)
  $("remove").onclick = () => show("remove-confirm", true);
  $("remove-no").onclick = () => show("remove-confirm", false);
  $("remove-yes").onclick = async () => {
    try { await invoke("profile_remove"); } catch (e) { note("acc-msg", String(e), "bad"); }
  };

  // ---------- Настройки ----------

  // «30 с», «1 мин», «2 мин» — без склонений, одинаково во всех языках
  const intervalName = (i) => (i < 60 ? T("int.s", { n: i }) : T("int.m", { n: i / 60 }));

  function fillSelect(sel, items, current, label) {
    sel.innerHTML = "";
    for (const v of items) {
      const o = document.createElement("option");
      o.value = v;
      o.textContent = label(v);
      if (String(v) === String(current)) o.selected = true;
      sel.appendChild(o);
    }
  }

  function renderViews() {
    const box = $("views");
    box.innerHTML = "";
    // Второй вид зависит от ОС (B2): панель задач Windows или строка меню macOS
    const all = ["overlay", st.os === "macos" ? "menubar" : "taskbar"];
    for (const v of all) {
      const b = document.createElement("button");
      b.type = "button";
      b.disabled = !st.views.includes(v);
      b.setAttribute("aria-pressed", String(st.view === v));
      b.textContent = T(VIEW_NAMES[v]);
      b.onclick = () => set({ view: v });
      box.appendChild(b);
    }
    // «Поверх окон» — только у плавающей панели; стиль строки меню — только на Mac
    show("on-top-row", st.view === "overlay");
    show("menubar-row", st.view === "menubar");
  }

  function applyView(v) {
    st = v;
    renderViews();
    $("on-top").checked = v.always_on_top;
    $("show-cat").checked = v.show_cat;
    $("cat-fact").checked = v.cat_fact;
    $("cat-fact").disabled = !v.show_cat;
    early.set(v);
    // Котофакт в превью панели — тот же, что на рабочем столе (закрыть можно и здесь)
    const E = TokenCat.early;
    panel.setCat(v.show_cat);
    panel.setFact(E.showFact({ enabled: v.cat_fact, cat: v.show_cat, hiddenDay: v.fact_hidden_day })
      ? E.factOfDay(TokenCat.catFacts()) : null,
      () => set({ fact_hidden_day: E.dayKey() }));
    $("notify").checked = v.notify;
    $("autostart").checked = v.autostart;
    $("auto-update").checked = v.auto_update;
    fillSelect($("interval"), v.intervals, v.interval_secs, intervalName);
    fillSelect($("language"), v.languages, v.language, langName);
    fillSelect($("menubar-style"), v.menubar_styles || [], v.menubar_style, (m) => (MENUBAR_NAMES[m] ? T(MENUBAR_NAMES[m]) : m));
    // Поле прокси не трогаем, пока в нём пишут
    if (document.activeElement !== $("proxy")) $("proxy").value = v.proxy || "";
    show("remove-row", !v.profile_default);
    $("version").textContent = v.version;
    document.title = v.profile_default ? "TokenCat — " + T("win.settings")
      : `TokenCat (${v.account_name || v.profile_id}) — ${T("win.settings")}`;
    render();
  }

  /** Отправить одно изменение. msgId — где показать ошибку. */
  async function set(patch, msgId = "sys-msg") {
    try {
      applyView(await invoke("settings_set", { patch }));
      note(msgId, "");
      return true;
    } catch (e) {
      note(msgId, String(e), "bad");
      // Вернуть переключатели как было
      try { applyView(await invoke("settings_get")); } catch (_) {}
      return false;
    }
  }

  $("on-top").onchange = (e) => set({ always_on_top: e.target.checked });
  $("menubar-style").onchange = (e) => set({ menubar_style: e.target.value });
  $("show-cat").onchange = (e) => set({ show_cat: e.target.checked });
  $("notify").onchange = (e) => set({ notify: e.target.checked });
  $("cat-fact").onchange = (e) => set({ cat_fact: e.target.checked });
  $("autostart").onchange = (e) => set({ autostart: e.target.checked });
  $("auto-update").onchange = (e) => set({ auto_update: e.target.checked });
  $("interval").onchange = (e) => set({ interval_secs: Number(e.target.value) });
  $("language").onchange = (e) => set({ language: e.target.value });
  $("notify-test").onclick = () => invoke("notify_test").catch((e) => note("sys-msg", String(e), "bad"));

  // Прокси: проверяем и применяем по кнопке или Enter
  async function saveProxy() {
    if (await set({ proxy: $("proxy").value }, "net-msg")) {
      note("net-msg", $("proxy").value.trim() ? T("net.applied") : T("net.off"), "ok");
    }
  }
  $("proxy-save").onclick = saveProxy;
  $("proxy").addEventListener("keydown", (e) => { if (e.key === "Enter") saveProxy(); });

  // Ручной ключ. Вставка Ctrl+V/Shift+Insert/ПКМ — штатная, работает при любой раскладке:
  // браузерный движок смотрит на физическую клавишу, а не на букву.
  $("manual-save").onclick = async () => {
    note("manual-msg", T("key.checking"));
    try {
      const masked = await invoke("auth_set_manual_key", { value: $("manual").value });
      $("manual").value = "";
      note("manual-msg", T("key.saved", { mask: masked }), "ok");
    } catch (e) {
      note("manual-msg", String(e), "bad");
    }
  };
  $("open-log").onclick = () => invoke("open_log").catch((e) => note("manual-msg", String(e), "bad"));
  $("onboarding").onclick = () => invoke("onboarding_open").catch(() => {});

  // ---------- Обновления (B4) ----------
  // Состояние держит Rust (updater.rs): idle | checking | none | available | downloading |
  // installing | error. Окно только показывает его и шлёт «проверить» / «обновить».

  function renderUpd(u) {
    if (!u) return;
    $("upd-current").textContent = u.current;
    const busy = ["checking", "downloading", "installing"].includes(u.state);
    $("upd-check").disabled = busy;
    let text = T("upd.idle");
    if (u.state === "checking") text = T("upd.checking");
    else if (u.state === "none") text = T("upd.none");
    else if (u.error && !u.version) text = u.error;
    $("upd-text").textContent = text;

    show("upd-avail", !!u.version);
    if (!u.version) return;
    $("upd-title").textContent = T("upd.available", { v: u.version });
    $("upd-notes").textContent = u.can_install ? (u.notes || "") : T("upd.portable_hint");
    $("upd-install").textContent = u.can_install ? T("upd.install") : T("upd.download");
    $("upd-install").disabled = busy;
    show("upd-progress", u.state === "downloading");
    $("upd-progress").value = u.progress || 0;
    let msg = "", cls = "muted";
    if (u.state === "downloading") msg = T("upd.downloading", { p: u.progress || 0 });
    else if (u.state === "installing") msg = T("upd.installing");
    else if (u.error) { msg = u.error; cls = "bad"; }
    note("upd-msg", msg, cls);
  }

  $("upd-check").onclick = async () => {
    try { renderUpd(await invoke("update_check")); } catch (e) { $("upd-text").textContent = String(e); }
  };
  $("upd-install").onclick = async () => {
    try { await invoke("update_install"); } catch (e) { note("upd-msg", String(e), "bad"); }
  };
  listen("update-changed", (ev) => renderUpd(ev.payload));
  invoke("update_status").then(renderUpd).catch(() => {});

  // Ссылки на донаты и обратную связь появятся в блоках B6–B7
  const soon = () => note("about-msg", T("about.soon"), "muted");
  $("donate").onclick = soon;
  $("report").onclick = soon;

  // Прокрутить к разделу (пункт «Поддержать проект» в меню)
  function goTo(section) {
    const el = section && $(section);
    if (!el) return;
    el.scrollIntoView({ block: "start", behavior: "smooth" });
    el.classList.remove("flash-target");
    void el.offsetWidth;
    el.classList.add("flash-target");
  }

  // ---------- События из Rust ----------

  listen("usage-updated", (ev) => {
    snap = ev.payload;
    early.setUsage(snap);
    // Планировщик заметил вход/выход раньше UI — перечитаем состояние входа
    const signedIn = snap.state !== "signed_out";
    if (signedIn !== auth.signed_in) loadAuth().then(render);
    else render();
  });
  listen("settings-changed", (ev) => applyView(ev.payload));
  listen("settings-goto", (ev) => goTo(ev.payload));
  listen("auth-changed", async (ev) => {
    const p = ev.payload || {};
    if (p.kind === "problem" || (p.kind === "cancelled" && p.message)) {
      panel.flash(p.message, "warn");
      return;
    }
    await loadAuth();
    render();
  });

  setInterval(() => panel.tick(), 1000);

  (async () => {
    await loadAuth();
    try { snap = await invoke("usage_snapshot"); early.setUsage(snap); } catch (_) {}
    try { applyView(await invoke("settings_get")); } catch (e) { note("sys-msg", String(e), "bad"); }
    render();
    try { goTo(await invoke("settings_goto")); } catch (_) {}
  })();
});
