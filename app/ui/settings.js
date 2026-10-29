// Временное окно настроек (S07, бывшее главное окно S04–S06). Данные приходят от планировщика
// в Rust (событие `usage-updated`), сам экран в сеть не ходит. Секрет в JS не попадает: Rust отдаёт только маску.
(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const show = (id, on) => { $(id).hidden = !on; };

  let snap = null;   // последний снимок от планировщика
  let auth = { signed_in: false };

  // Общая панель из components.js (S06): строки лимитов, статус, кнопка входа/лога
  const panel = new TokenCat.ui.Panel({
    breakdown: true,
    onAction: (id) => {
      if (id === "login") login();
      else if (id === "log") invoke("open_log").catch((e) => panel.flash(String(e), "warn"));
    },
  });
  $("panel-slot").appendChild(panel.root);

  // Снимок планировщика + состояние входа → то, что показывает панель
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
    $("account").textContent = [s.org_name, auth.masked].filter(Boolean).join(" · ");
    show("account", signedIn && (s.org_name || auth.masked));
    show("refresh", signedIn);
    $("refresh").disabled = !!s.fetching;
    show("logout", auth.signed_in);
  }

  async function loadAuth() {
    try { auth = await invoke("auth_status"); }
    catch (e) { auth = { signed_in: false, error: String(e) }; }
  }

  async function loadAll() {
    await loadAuth();
    try { snap = await invoke("usage_snapshot"); } catch (_) {}
    render();
  }

  async function login() {
    panel.flash("Войдите в открывшемся окне claude.ai…");
    try { await invoke("auth_login"); } catch (e) { panel.flash(String(e), "warn"); }
  }
  $("refresh").onclick = () => invoke("usage_refresh").catch(() => {});
  $("logout").onclick = async () => {
    try { await invoke("auth_logout"); } catch (e) { panel.flash(String(e), "warn"); }
  };
  // Настройки сети
  async function loadSettings() {
    try {
      const s = await invoke("settings_get");
      const sel = $("interval");
      sel.innerHTML = "";
      for (const i of s.intervals) {
        const o = document.createElement("option");
        o.value = i;
        o.textContent = i < 60 ? `${i} секунд` : i === 60 ? "минуту" : `${i / 60} минуты`.replace("5 минуты", "5 минут");
        if (i === s.interval_secs) o.selected = true;
        sel.appendChild(o);
      }
      $("proxy").value = s.proxy || "";
    } catch (_) {}
  }
  $("net-save").onclick = async () => {
    const msg = $("net-msg");
    msg.hidden = false;
    try {
      const s = await invoke("settings_set", {
        intervalSecs: Number($("interval").value),
        proxy: $("proxy").value,
      });
      $("proxy").value = s.proxy || "";
      msg.className = "small ok";
      msg.textContent = "Сохранено";
    } catch (e) {
      msg.className = "small bad";
      msg.textContent = String(e);
    }
  };

  // Ручной ключ. Вставка Ctrl+V/Shift+Insert/ПКМ — штатная, работает при любой раскладке:
  // браузерный движок смотрит на физическую клавишу, а не на букву.
  $("manual-save").onclick = async () => {
    const msg = $("manual-msg");
    msg.hidden = false;
    msg.className = "small muted";
    msg.textContent = "Проверяю ключ…";
    try {
      const masked = await invoke("auth_set_manual_key", { value: $("manual").value });
      $("manual").value = "";
      msg.className = "small ok";
      msg.textContent = "Сохранено: " + masked;
      $("advanced").open = false;
    } catch (e) {
      msg.className = "small bad";
      msg.textContent = String(e);
    }
  };

  // Данные от планировщика
  listen("usage-updated", (ev) => {
    snap = ev.payload;
    // Планировщик заметил вход/выход раньше UI — перечитаем состояние входа
    const signedIn = snap.state !== "signed_out";
    if (signedIn !== auth.signed_in) loadAuth().then(render);
    else render();
  });

  // События входа: вход завершён, выход, окно входа закрыли, проблема (VPN и т.п.)
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
  loadSettings();
  loadAll();
})();
