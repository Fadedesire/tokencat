// Временный экран S04–S05. Данные приходят от планировщика в Rust (событие `usage-updated`),
// сам экран в сеть не ходит. Секрет в JS не попадает: Rust отдаёт только маску.
(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const show = (id, on) => { $(id).hidden = !on; };

  let snap = null;   // последний снимок от планировщика
  let auth = { signed_in: false };

  const LABELS = {
    session: "5 часов",
    weekly_all: "Неделя",
    weekly_sonnet: "Неделя · Sonnet",
    weekly_opus: "Неделя · Opus",
  };

  function fmtLeft(ms) {
    const s = Math.max(0, Math.round(ms / 1000));
    const d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60);
    if (d > 0) return `${d} д ${h} ч`;
    if (h > 0) return `${h} ч ${m} мин`;
    if (m > 0) return `${m} мин`;
    return `${s} с`;
  }

  function fmtReset(iso) {
    if (!iso) return "";
    const ms = new Date(iso) - Date.now();
    if (!(ms > 0)) return "сброс скоро";
    return "сброс через " + fmtLeft(ms);
  }

  function fmtAgo(ms) {
    const s = Math.round((Date.now() - ms) / 1000);
    if (s < 5) return "только что";
    return fmtLeft(s * 1000) + " назад";
  }

  function limitRows(u) {
    if (!u) return [];
    return [u.session, u.weekly, ...(u.others || [])].filter(Boolean);
  }

  function renderLimits() {
    const ul = $("limits");
    ul.innerHTML = "";
    const rows = limitRows(snap && snap.usage);
    const stale = snap && snap.state !== "ok";
    for (const l of rows) {
      const li = document.createElement("li");
      if (stale) li.className = "stale";
      const lvl = l.percent >= 80 ? "red" : l.percent >= 50 ? "yellow" : "green";
      li.innerHTML = `<span class="label"></span><span class="bar"><i class="${lvl}"></i></span>` +
        `<span class="pct"></span><span class="reset muted small"></span>`;
      li.querySelector(".label").textContent = LABELS[l.kind] || l.kind;
      li.querySelector(".bar i").style.width = Math.min(100, Math.max(0, l.percent)) + "%";
      li.querySelector(".pct").textContent = Math.round(l.percent) + "%";
      li.querySelector(".reset").dataset.iso = l.resets_at || "";
      ul.appendChild(li);
    }
    // Доля недели по продуктам — одной строкой под лимитами
    const parts = ((snap && snap.usage && snap.usage.breakdown) || []).filter((b) => b.percent > 0);
    if (parts.length) {
      const li = document.createElement("li");
      li.innerHTML = `<span class="breakdown muted small"></span>`;
      li.querySelector(".breakdown").textContent =
        "Неделя: " + parts.map((b) => `${b.name} ${Math.round(b.percent)}%`).join(" · ");
      ul.appendChild(li);
    }
    show("limits", rows.length > 0);
  }

  // Тикает раз в секунду локально, без запросов
  function renderTimes() {
    for (const el of document.querySelectorAll(".limits .reset")) {
      el.textContent = fmtReset(el.dataset.iso);
    }
    const st = $("status");
    if (!snap || !auth.signed_in) { st.textContent = ""; return; }
    const parts = [];
    if (snap.fetching) parts.push("обновляю…");
    else if (snap.updated_at_ms) parts.push((snap.state === "ok" ? "обновлено " : "данные от ") + fmtAgo(snap.updated_at_ms));
    if (!snap.fetching && snap.next_at_ms) parts.push("следующее через " + fmtLeft(snap.next_at_ms - Date.now()));
    st.textContent = parts.join(" · ");
    st.className = "muted small status" + (snap.state !== "ok" && snap.usage ? " stale" : "");
  }

  function render() {
    const s = snap || { state: "loading", message: "Загружаю данные…" };
    const signedIn = auth.signed_in && s.state !== "signed_out";
    const state = auth.error ? "error" : !auth.signed_in ? "signed_out" : s.state;
    $("message").textContent = auth.error || (!auth.signed_in ? "Вы не вошли в claude.ai" : s.message);
    $("message").className = "message " + state;
    $("account").textContent = [s.org_name, auth.masked].filter(Boolean).join(" · ");
    show("account", signedIn && (s.org_name || auth.masked));

    renderLimits();
    renderTimes();

    show("log-link", !!s.show_log);
    show("login", state === "signed_out" || state === "expired");
    $("login").textContent = state === "expired" ? "Войти заново" : "Войти в Claude";
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

  $("login").onclick = async () => {
    $("message").textContent = "Войдите в открывшемся окне claude.ai…";
    try { await invoke("auth_login"); } catch (e) { $("message").textContent = String(e); }
  };
  $("refresh").onclick = () => invoke("usage_refresh").catch(() => {});
  $("logout").onclick = async () => {
    try { await invoke("auth_logout"); } catch (e) { $("message").textContent = String(e); }
  };
  $("open-log").onclick = (ev) => {
    ev.preventDefault();
    invoke("open_log").catch((e) => { $("message").textContent = String(e); });
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
      $("message").textContent = p.message;
      return;
    }
    await loadAuth();
    render();
  });

  setInterval(renderTimes, 1000);
  loadSettings();
  loadAll();
})();
