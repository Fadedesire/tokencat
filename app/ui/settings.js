// Окно настроек (B1, по SPEC). Данные приходят от Rust: снимок планировщика (`usage-updated`),
// настройки (`settings-changed`), вход (`auth-changed`). Сам экран в сеть не ходит.
// Секрет в JS не попадает: Rust отдаёт только маску ключа.
// Всё применяется сразу: каждый переключатель отправляет одно поле в settings_set.
(function () {
  "use strict";
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const show = (id, on) => { $(id).hidden = !on; };

  const LANG_NAMES = {
    system: "Как в системе", ru: "Русский", en: "English", es: "Español",
    "pt-BR": "Português (Brasil)", de: "Deutsch", fr: "Français", ja: "日本語",
    ko: "한국어", "zh-Hans": "简体中文",
  };
  const VIEW_NAMES = {
    overlay: "Плавающая панель",
    taskbar: "В панели задач",
    menubar: "В строке меню",
  };

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

  function note(id, text, cls) {
    const el = $(id);
    el.hidden = !text;
    el.className = (el.classList.contains("item") ? "msg item " : "msg ") + (cls || "muted");
    el.textContent = text || "";
  }

  // ---------- Аккаунт и панель ----------

  function viewSnap() {
    const s = snap || { state: "loading", message: "Загружаю данные…" };
    if (auth.error) return { ...s, state: "error", message: auth.error, usage: null };
    if (!auth.signed_in) return { state: "signed_out", message: "Вы не вошли в claude.ai" };
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
    $("acc-name").textContent = auth.signed_in ? name || "Вы вошли в claude.ai" : "Не вошли";
    $("acc-hint").textContent = auth.signed_in
      ? "Имя берём из профиля claude.ai. Почту не показываем."
      : "Нажмите «Войти в Claude» — откроется окно claude.ai.";
  }

  async function loadAuth() {
    try { auth = await invoke("auth_status"); }
    catch (e) { auth = { signed_in: false, error: String(e) }; }
  }

  async function login() {
    panel.flash("Войдите в открывшемся окне claude.ai…");
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
      note("acc-msg", "Открываю вторую панель — войдите в ней в другой аккаунт.", "ok");
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

  function intervalName(i) {
    if (i < 60) return `${i} секунд`;
    if (i === 60) return "минуту";
    const m = i / 60;
    return m >= 5 ? `${m} минут` : `${m} минуты`;
  }

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
    // Второй вид зависит от ОС; пока он «скоро» (блок B2)
    const all = ["overlay", st.os === "macos" ? "menubar" : "taskbar"];
    for (const v of all) {
      const b = document.createElement("button");
      b.type = "button";
      const ready = st.views.includes(v);
      b.disabled = !ready;
      b.setAttribute("aria-pressed", String(st.view === v));
      b.textContent = VIEW_NAMES[v] + " ";
      if (!ready) {
        const s = document.createElement("span");
        s.className = "soon";
        s.textContent = "скоро";
        b.appendChild(s);
      }
      b.onclick = () => set({ view: v });
      box.appendChild(b);
    }
  }

  function applyView(v) {
    st = v;
    renderViews();
    $("on-top").checked = v.always_on_top;
    $("show-cat").checked = v.show_cat;
    $("notify").checked = v.notify;
    $("autostart").checked = v.autostart;
    fillSelect($("interval"), v.intervals, v.interval_secs, intervalName);
    fillSelect($("language"), v.languages, v.language, (l) => LANG_NAMES[l] || l);
    // Поле прокси не трогаем, пока в нём пишут
    if (document.activeElement !== $("proxy")) $("proxy").value = v.proxy || "";
    show("remove-row", !v.profile_default);
    $("version").textContent = v.version;
    document.title = v.profile_default ? "TokenCat — настройки"
      : `TokenCat (${v.account_name || v.profile_id}) — настройки`;
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
  $("show-cat").onchange = (e) => set({ show_cat: e.target.checked });
  $("notify").onchange = (e) => set({ notify: e.target.checked });
  $("autostart").onchange = (e) => set({ autostart: e.target.checked });
  $("interval").onchange = (e) => set({ interval_secs: Number(e.target.value) });
  $("language").onchange = (e) => set({ language: e.target.value });
  $("notify-test").onclick = () => invoke("notify_test").catch((e) => note("sys-msg", String(e), "bad"));

  // Прокси: проверяем и применяем по кнопке или Enter
  async function saveProxy() {
    if (await set({ proxy: $("proxy").value }, "net-msg")) {
      note("net-msg", $("proxy").value.trim() ? "Прокси применён, проверяю связь…" : "Прокси выключен", "ok");
    }
  }
  $("proxy-save").onclick = saveProxy;
  $("proxy").addEventListener("keydown", (e) => { if (e.key === "Enter") saveProxy(); });

  // Ручной ключ. Вставка Ctrl+V/Shift+Insert/ПКМ — штатная, работает при любой раскладке:
  // браузерный движок смотрит на физическую клавишу, а не на букву.
  $("manual-save").onclick = async () => {
    note("manual-msg", "Проверяю ключ…");
    try {
      const masked = await invoke("auth_set_manual_key", { value: $("manual").value });
      $("manual").value = "";
      note("manual-msg", "Сохранено: " + masked, "ok");
    } catch (e) {
      note("manual-msg", String(e), "bad");
    }
  };
  $("open-log").onclick = () => invoke("open_log").catch((e) => note("manual-msg", String(e), "bad"));
  $("onboarding").onclick = () => invoke("onboarding_open").catch(() => {});

  // Ссылки на донаты и обратную связь появятся в блоках B6–B7
  const soon = () => note("about-msg", "Ссылки появятся к выходу первой версии. Спасибо!", "muted");
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
    try { snap = await invoke("usage_snapshot"); } catch (_) {}
    try { applyView(await invoke("settings_get")); } catch (e) { note("sys-msg", String(e), "bad"); }
    render();
    try { goTo(await invoke("settings_goto")); } catch (_) {}
  })();
})();
