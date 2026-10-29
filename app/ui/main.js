// Вид «Поверх окон» (S07) — главное окно TokenCat.
// Панель из components.js; данные приходят от планировщика в Rust (событие `usage-updated`).
//   ЛКМ + тащить — двигать панель (позицию запоминает Rust);
//   двойной клик — обновить сейчас;
//   ПКМ — меню (Обновить, Настройки, Поверх окон, Вид, Аккаунты, Поддержать, Выход) —
//   нативное, общее со значком в трее (Rust, tray.rs).
// В шапке — имя аккаунта claude.ai (B1), котик выключается в настройках.
(function () {
  "use strict";
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const win = window.__TAURI__.window.getCurrentWindow();

  let snap = null;
  let auth = { signed_in: false };

  const panel = new TokenCat.ui.Panel({
    onAction: (id) => {
      if (id === "login") {
        panel.flash("Войдите в открывшемся окне claude.ai…");
        invoke("auth_login").catch((e) => panel.flash(String(e), "warn"));
      } else if (id === "log") {
        invoke("open_log").catch((e) => panel.flash(String(e), "warn"));
      }
    },
  });
  document.getElementById("slot").appendChild(panel.root);

  // Снимок планировщика + состояние входа → что показать (как в окне настроек)
  function viewSnap() {
    const s = snap || { state: "loading", message: "Загружаю данные…" };
    if (auth.error) return { ...s, state: "error", message: auth.error, usage: null };
    if (!auth.signed_in) return { state: "signed_out", message: "Вы не вошли в claude.ai" };
    return s;
  }
  const render = () => {
    const v = viewSnap();
    panel.update(v);
    panel.setTitle(auth.signed_in ? v.account_name : null);
    fit();
  };

  // Настройки, которые видны на панели: котик
  function applySettings(st) {
    if (!st) return;
    panel.setCat(st.show_cat !== false);
    fit();
  }

  async function loadAuth() {
    try { auth = await invoke("auth_status"); }
    catch (e) { auth = { signed_in: false, error: String(e) }; }
  }

  // ---- Размер окна = размер панели ----
  let lastW = 0, lastH = 0;
  function fit() {
    const r = panel.root.getBoundingClientRect();
    const w = Math.ceil(r.width), h = Math.ceil(r.height);
    if (!w || !h || (w === lastW && h === lastH)) return;
    lastW = w; lastH = h;
    invoke("overlay_fit", { width: w, height: h }).catch(() => {});
  }
  if (window.ResizeObserver) new ResizeObserver(fit).observe(panel.root);

  // ---- Мышь ----
  let lastRefresh = 0;
  function refreshNow() {
    const t = Date.now();
    if (t - lastRefresh < 600) return;   // mousedown(detail=2) и dblclick — один раз
    lastRefresh = t;
    invoke("usage_refresh").catch(() => {});
  }
  panel.root.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || e.target.closest("button")) return;
    if (e.detail === 2) { refreshNow(); return; }
    win.startDragging().catch(() => {});
  });
  // Запасной путь для двойного клика (если система отдала его отдельным событием)
  panel.root.addEventListener("dblclick", (e) => {
    if (!e.target.closest("button")) refreshNow();
  });
  document.addEventListener("contextmenu", (e) => {
    e.preventDefault();               // вместо меню браузера — своё
    invoke("overlay_menu").catch(() => {});
  });

  // ---- События из Rust ----
  listen("usage-updated", (ev) => {
    snap = ev.payload;
    const signedIn = snap.state !== "signed_out";
    if (signedIn !== auth.signed_in) loadAuth().then(render);
    else render();
  });
  listen("settings-changed", (ev) => applySettings(ev.payload));
  listen("auth-changed", async (ev) => {
    const p = ev.payload || {};
    if (p.kind === "problem" || (p.kind === "cancelled" && p.message)) {
      panel.flash(p.message, "warn");
      fit();
      return;
    }
    await loadAuth();
    render();
  });

  setInterval(() => panel.tick(), 1000);

  (async () => {
    try { applySettings(await invoke("settings_get")); } catch (_) {}
    await loadAuth();
    try { snap = await invoke("usage_snapshot"); } catch (_) {}
    render();
  })();
})();
