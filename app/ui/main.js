// Временный экран S04: состояние входа, кнопки «Войти»/«Выйти», ручной ключ.
// Секрет в JS не попадает: Rust отдаёт только маску.
(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const show = (id, on) => { $(id).hidden = !on; };

  let busy = false;

  function fmtReset(iso) {
    if (!iso) return "";
    const ms = new Date(iso) - Date.now();
    if (!(ms > 0)) return "сброс скоро";
    const h = Math.floor(ms / 3600000), m = Math.floor((ms % 3600000) / 60000);
    if (h >= 24) return `сброс через ${Math.floor(h / 24)} д ${h % 24} ч`;
    return h > 0 ? `сброс через ${h} ч ${m} мин` : `сброс через ${m} мин`;
  }

  function render(s) {
    const signedIn = s.state === "signed_in";
    const hasKey = s.state !== "signed_out";
    $("message").textContent = s.message;
    $("message").className = "message " + s.state;
    $("account").textContent = [s.org_name, s.masked].filter(Boolean).join(" · ");
    show("account", hasKey && (s.org_name || s.masked));

    const ul = $("limits");
    ul.innerHTML = "";
    for (const l of s.limits || []) {
      const li = document.createElement("li");
      const lvl = l.percent >= 80 ? "red" : l.percent >= 50 ? "yellow" : "green";
      li.innerHTML = `<span class="label"></span><span class="bar"><i class="${lvl}"></i></span>` +
        `<span class="pct"></span><span class="reset muted small"></span>`;
      li.querySelector(".label").textContent = l.label;
      li.querySelector(".bar i").style.width = Math.min(100, Math.max(0, l.percent)) + "%";
      li.querySelector(".pct").textContent = Math.round(l.percent) + "%";
      li.querySelector(".reset").textContent = fmtReset(l.resets_at);
      ul.appendChild(li);
    }
    show("limits", signedIn && s.limits && s.limits.length);

    // «Войти» — когда не вошли или сессия истекла
    show("login", s.state === "signed_out" || s.state === "expired");
    $("login").textContent = s.state === "expired" ? "Войти заново" : "Войти в Claude";
    show("refresh", hasKey);
    show("logout", hasKey);
  }

  async function refresh() {
    if (busy) return;
    busy = true;
    $("refresh").disabled = true;
    try {
      render(await invoke("auth_status"));
    } catch (e) {
      render({ state: "error", message: String(e), limits: [] });
    } finally {
      busy = false;
      $("refresh").disabled = false;
    }
  }

  $("login").onclick = async () => {
    $("message").textContent = "Войдите в открывшемся окне claude.ai…";
    try { await invoke("auth_login"); } catch (e) { $("message").textContent = String(e); }
  };
  $("refresh").onclick = refresh;
  $("logout").onclick = async () => {
    try { await invoke("auth_logout"); } catch (e) { $("message").textContent = String(e); }
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

  // События из Rust: вход завершён, выход, окно входа закрыли, проблема (VPN и т.п.)
  listen("auth-changed", (ev) => {
    const p = ev.payload || {};
    if (p.kind === "problem" || (p.kind === "cancelled" && p.message)) {
      $("message").textContent = p.message;
      return;
    }
    refresh();
  });

  refresh();
})();
