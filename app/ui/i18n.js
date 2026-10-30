// Языки интерфейса (B3). Словарь приходит из Rust (команда `i18n_get`, файлы app/locales/*.json),
// здесь — только поиск строки и подстановки. Без Tauri (в Node-тестах) словарь задают load().
//
//   T("key")                — строка; нет в словаре — сам ключ (так пропуски видны сразу)
//   T("key", { n: 5 })      — подстановки {n}
//   <b data-i18n="key">     — текст элемента; data-i18n-html — разметка (только из наших файлов);
//   data-i18n-title / data-i18n-ph — подсказка и placeholder.
// Пока словарь грузится, страница скрыта (класс i18n на <html>, см. tokens.css) — без мигания
// русского текста. Сменили язык — Rust шлёт `lang-changed`, окно перезагружается.
(function (root) {
  "use strict";
  let dict = {};
  let lang = "en";

  function T(key, args) {
    let s = dict[key];
    if (typeof s !== "string") s = key;
    if (args) s = s.replace(/\{(\w+)\}/g, (m, k) => (k in args ? String(args[k]) : m));
    return s;
  }

  function load(d, l) {
    dict = d || {};
    lang = l || "en";
  }

  /** Подставить переводы в разметку внутри el. */
  function apply(el) {
    const q = (sel, fn) => el.querySelectorAll(sel).forEach(fn);
    q("[data-i18n]", (e) => { e.textContent = T(e.dataset.i18n); });
    q("[data-i18n-html]", (e) => { e.innerHTML = T(e.dataset.i18nHtml); });
    q("[data-i18n-title]", (e) => { e.title = T(e.dataset.i18nTitle); });
    q("[data-i18n-ph]", (e) => { e.placeholder = T(e.dataset.i18nPh); });
  }

  // ---- Форматы времени ----
  const pad = (n) => String(n).padStart(2, "0");

  /** Время до момента iso: «4ч 05м», «3д 4ч», «45м», «<1м», «сейчас» — по словарю языка. */
  function until(iso, now = Date.now()) {
    if (!iso) return "";
    const t = Date.parse(iso);
    if (isNaN(t)) return "";
    const sec = Math.floor((t - now) / 1000);
    if (sec <= 0) return T("dur.now");
    let m = Math.floor(sec / 60);
    const h = Math.floor(m / 60) % 24, d = Math.floor(m / 1440);
    m %= 60;
    if (d) return T("dur.dh", { d, h });
    if (h) return T("dur.hm", { h, m: pad(m) });
    return m ? T("dur.m", { m }) : T("dur.lt1m");
  }

  const api = {
    T, load, apply, until,
    get lang() { return lang; },
    get dict() { return dict; },
    ready: null,
  };

  const tauri = root.__TAURI__;
  if (tauri && root.document) {
    const de = root.document.documentElement;
    api.ready = tauri.core.invoke("i18n_get")
      .then((r) => { load(r.dict, r.lang); })
      .catch(() => {})
      .then(() => {
        de.lang = lang;
        apply(root.document);
        de.classList.remove("i18n");
      });
    tauri.event.listen("lang-changed", () => root.location.reload());
  } else {
    api.ready = Promise.resolve();
  }

  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else {
    const TC = (root.TokenCat = root.TokenCat || {});
    TC.i18n = api;
    TC.T = T;
  }
})(typeof window !== "undefined" ? window : globalThis);
