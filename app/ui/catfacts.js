// Котофакт дня (B9). Сами факты теперь в словаре языка (app/locales/*.json, ключ facts) —
// на каждом языке свои 60 фактов, каждый короче 200 символов. Выбор факта на день — early.js.
(function (root) {
  "use strict";
  const TC = (root.TokenCat = root.TokenCat || {});
  TC.catFacts = () => {
    const f = TC.i18n && TC.i18n.dict && TC.i18n.dict.facts;
    return Array.isArray(f) ? f : [];
  };
})(typeof window !== "undefined" ? window : globalThis);
