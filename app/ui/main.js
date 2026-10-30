// Вид «Поверх окон» (S07) — главное окно TokenCat.
// Панель из components.js; данные приходят от планировщика в Rust (событие `usage-updated`).
//   ЛКМ + тащить — двигать панель (позицию запоминает Rust);
//   двойной клик — обновить сейчас;
//   ПКМ — меню (Обновить, Настройки, Поверх окон, Вид, Аккаунты, Поддержать, Выход) —
//   нативное, общее со значком в трее (Rust, tray.rs).
// В шапке — имя аккаунта claude.ai (B1), котик выключается в настройках.
// Виды B2 («В панели задач» / «В строке меню»): это же окно — всплывающая панель у виджета.
//   Её не таскают; прячется сама, когда щёлкнули мимо (Rust), или по Esc.
TokenCat.i18n.ready.then(function () {
  "use strict";
  const T = TokenCat.T;
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const win = window.__TAURI__.window.getCurrentWindow();

  let snap = null;
  let auth = { signed_in: false };
  let popup = false;      // вид не «Поверх окон» — панель всплывающая

  const panel = new TokenCat.ui.Panel({
    onAction: (id) => {
      if (id === "login") {
        panel.flash(T("login.wait"));
        invoke("auth_login").catch((e) => panel.flash(String(e), "warn"));
      } else if (id === "log") {
        invoke("open_log").catch((e) => panel.flash(String(e), "warn"));
      }
    },
  });
  const slot = document.getElementById("slot");
  slot.appendChild(panel.root);

  // Снимок планировщика + состояние входа → что показать (как в окне настроек)
  function viewSnap() {
    const s = snap || { state: "loading", message: T("panel.loading") };
    if (auth.error) return { ...s, state: "error", message: auth.error, usage: null };
    if (!auth.signed_in) return { state: "signed_out", message: T("panel.signed_out") };
    return s;
  }
  const render = () => {
    const v = viewSnap();
    panel.update(v);
    panel.setTitle(auth.signed_in ? v.account_name : null);
    fit();
  };

  // Настройки, которые видны на панели: котик, котофакт
  let settings = null;
  function applySettings(st) {
    if (!st) return;
    settings = st;
    popup = st.view !== "overlay";
    document.body.classList.toggle("popup", popup);
    panel.setCat(st.show_cat !== false);
    updateFact();
    fit();
  }

  // ---- Котофакт дня (B9): облачко раз в день, клик — спрятать до завтра ----
  const early = TokenCat.early;
  function updateFact() {
    const st = settings;
    const show = st && early && early.showFact({
      enabled: st.cat_fact, cat: st.show_cat !== false, hiddenDay: st.fact_hidden_day,
    });
    if (!show) { panel.setFact(null); fit(); return; }
    panel.setFact(early.factOfDay(TokenCat.catFacts()), () => {
      fit();
      invoke("settings_set", { patch: { fact_hidden_day: early.dayKey() } }).catch(() => {});
    });
    fit();
  }
  // Новый день наступил — показать новый факт (проверка раз в минуту, без запросов)
  let factDay = early ? early.dayKey() : "";
  setInterval(() => {
    if (!early) return;
    const d = early.dayKey();
    if (d !== factDay) { factDay = d; updateFact(); }
  }, 60e3);

  async function loadAuth() {
    try { auth = await invoke("auth_status"); }
    catch (e) { auth = { signed_in: false, error: String(e) }; }
  }

  // ---- Размер окна = размер панели ----
  let lastW = 0, lastH = 0;
  function fit() {
    // Во время залёта/вылета панель сжата трансформом — размер окна не трогаем
    if (slot.classList.contains("fly-in") || slot.classList.contains("fly-out")) return;
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
    if (!popup) win.startDragging().catch(() => {});
  });
  // Запасной путь для двойного клика (если система отдала его отдельным событием)
  panel.root.addEventListener("dblclick", (e) => {
    if (!e.target.closest("button")) refreshNow();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && popup) win.hide().catch(() => {});
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
  // Перетаскивание в панель задач и обратно (B2, анимации B10).
  // Залёт: панель втягивается в точку (x, y) окна и гаснет, потом Rust прячет окно.
  // Вылет: панель «выплёскивается» из точки, за которую её держат. Начинается с прозрачной —
  // это прикрывает первые кадры, пока WebView дорисовывает только что показанное окно.
  // Погасшая панель остаётся погасшей до следующего показа: иначе Windows на кадр покажет
  // её старую картинку. Через два кадра после показа сообщаем Rust «нарисована».
  // «Эффекты анимации» Windows выключены → prefers-reduced-motion → без анимаций.
  const reduced = () => !!(window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches);
  function fly(cls, x, y) {
    const dpr = window.devicePixelRatio || 1;
    slot.style.transformOrigin = (x / dpr) + "px " + (y / dpr) + "px";
    slot.classList.remove("fly-in", "fly-out");
    void slot.offsetWidth;            // перезапустить анимацию
    slot.classList.add(cls);
  }
  slot.addEventListener("animationend", (e) => {
    if (e.target === slot && e.animationName === "tc-fly-out") {
      slot.classList.remove("fly-out");
      fit();
    }
  });
  listen("panel-fly-in", (ev) => {
    const p = ev.payload || {};
    if (!p.anim || reduced()) { document.body.style.opacity = "0"; return; }
    fly("fly-in", p.x, p.y);
  });
  listen("panel-appear", (ev) => {
    const p = ev.payload;
    document.body.style.opacity = "";
    slot.classList.remove("fly-in");
    if (p && p.fly && !reduced()) fly("fly-out", p.fly[0], p.fly[1]);
    else { slot.classList.remove("fly-out"); fit(); }
    requestAnimationFrame(() => requestAnimationFrame(() => {
      invoke("panel_painted").catch(() => {});
    }));
  });
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
});
