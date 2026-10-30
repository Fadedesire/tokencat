// Языки интерфейса (B3). Один источник строк на всё: файлы `app/locales/<язык>.json`
// зашиты в exe (include_str!). Rust берёт из них меню трея, уведомления, ошибки и сообщения
// панели, окна получают весь словарь командой `i18n_get` (ui/i18n.js).
//
// Формат файла: плоский объект «ключ → строка», подстановки — `{имя}`. Особый ключ `facts` —
// массив котофактов. Чего нет в выбранном языке, берём из английского, а нет и там — ключ.
// Язык из настроек: "system" — язык системы (если он из девяти), иначе английский.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::RwLock;

use serde_json::{Map, Value};

/// Код языка → файл. Порядок — как в списке настроек (settings::LANGS без "system").
const FILES: [(&str, &str); 9] = [
    ("ru", include_str!("../../locales/ru.json")),
    ("en", include_str!("../../locales/en.json")),
    ("es", include_str!("../../locales/es.json")),
    ("pt-BR", include_str!("../../locales/pt-BR.json")),
    ("de", include_str!("../../locales/de.json")),
    ("fr", include_str!("../../locales/fr.json")),
    ("ja", include_str!("../../locales/ja.json")),
    ("ko", include_str!("../../locales/ko.json")),
    ("zh-Hans", include_str!("../../locales/zh-Hans.json")),
];

pub const FALLBACK: &str = "en";

/// Язык до init(): в тестах — русский (на нём написаны проверки текстов), в программе — EN.
#[cfg(test)]
const DEFAULT: &str = "ru";
#[cfg(not(test))]
const DEFAULT: &str = FALLBACK;

struct Current {
    lang: &'static str,
    /// Выбранный язык поверх английского — готовый словарь
    map: HashMap<String, Value>,
}

static CUR: RwLock<Option<Current>> = RwLock::new(None);

fn parse(lang: &str) -> Map<String, Value> {
    let Some((_, text)) = FILES.iter().find(|(l, _)| *l == lang) else { return Map::new() };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Словарь языка с английским под ним (для `i18n_get` и для CUR).
pub fn merged(lang: &str) -> Map<String, Value> {
    let mut m = parse(FALLBACK);
    if lang != FALLBACK {
        for (k, v) in parse(lang) {
            m.insert(k, v);
        }
    }
    m
}

/// Код из списка или None.
fn known(code: &str) -> Option<&'static str> {
    FILES.iter().map(|(l, _)| *l).find(|l| *l == code)
}

/// Тег языка системы («ru-RU», «pt_BR», «zh-Hant-TW», «en») → наш язык.
/// Португальский — только бразильский вариант; китайский — упрощённый для всех вариантов.
pub fn match_tag(tag: &str) -> Option<&'static str> {
    let t = tag.trim().replace('_', "-").to_ascii_lowercase();
    let base = t.split(['-', '.', '@']).next().unwrap_or("");
    match base {
        "pt" => Some("pt-BR"),
        "zh" => Some("zh-Hans"),
        "" => None,
        b => known(b),
    }
}

/// Язык системы из списка предпочтений; ничего не подошло — английский.
pub fn system_lang() -> &'static str {
    sys_locale::get_locales().find_map(|t| match_tag(&t)).unwrap_or(FALLBACK)
}

/// Настройка ("system" | код) → код языка.
pub fn resolve(setting: &str) -> &'static str {
    known(setting).unwrap_or_else(system_lang)
}

/// Включить язык по настройке. Возвращает код, если язык сменился.
pub fn set(setting: &str) -> Option<&'static str> {
    let lang = resolve(setting);
    let mut cur = CUR.write().unwrap();
    if cur.as_ref().is_some_and(|c| c.lang == lang) {
        return None;
    }
    *cur = Some(Current { lang, map: merged(lang).into_iter().collect() });
    Some(lang)
}

/// Текущий язык.
pub fn lang() -> &'static str {
    CUR.read().unwrap().as_ref().map(|c| c.lang).unwrap_or(DEFAULT)
}

fn lookup(key: &str) -> String {
    {
        let cur = CUR.read().unwrap();
        if let Some(c) = cur.as_ref() {
            return c.map.get(key).and_then(Value::as_str).unwrap_or(key).to_string();
        }
    }
    // До init (и в тестах) — без кеша
    merged(DEFAULT).get(key).and_then(Value::as_str).unwrap_or(key).to_string()
}

/// Строка по ключу.
pub fn t(key: &str) -> String {
    lookup(key)
}

/// Строка с подстановками: tf("alert.x", &[("n", &5)]) для «… {n} …».
pub fn tf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    fill(&lookup(key), args)
}

pub fn fill(s: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = s.to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), &v.to_string());
    }
    out
}

/// Все коды языков, у которых есть файл.
#[cfg(test)]
fn codes() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(l, _)| *l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn tags_of_systems() {
        assert_eq!(match_tag("ru-RU"), Some("ru"));
        assert_eq!(match_tag("en_US.UTF-8"), Some("en"));
        assert_eq!(match_tag("pt-PT"), Some("pt-BR"));
        assert_eq!(match_tag("pt_BR"), Some("pt-BR"));
        assert_eq!(match_tag("zh-Hant-TW"), Some("zh-Hans"));
        assert_eq!(match_tag("zh-CN"), Some("zh-Hans"));
        assert_eq!(match_tag("ja"), Some("ja"));
        assert_eq!(match_tag("DE-de"), Some("de"));
        assert_eq!(match_tag("uk-UA"), None);
        assert_eq!(match_tag(""), None);
        assert_eq!(resolve("fr"), "fr");
        assert_eq!(resolve("ko"), "ko");
    }

    /// Все файлы — корректный JSON, ключи совпадают с английскими, подстановки те же,
    /// котофактов столько же и каждый короче 200 символов.
    #[test]
    fn locales_complete() {
        let en = parse("en");
        assert!(en.len() > 100, "в en.json мало строк");
        let keys = |m: &Map<String, Value>| m.keys().cloned().collect::<BTreeSet<_>>();
        let vars = |s: &str| {
            let mut v: Vec<String> = Vec::new();
            let mut rest = s;
            while let Some(a) = rest.find('{') {
                let Some(b) = rest[a..].find('}') else { break };
                v.push(rest[a + 1..a + b].to_string());
                rest = &rest[a + b + 1..];
            }
            v.sort();
            v
        };
        for code in codes() {
            let m = parse(code);
            assert!(!m.is_empty(), "{code}.json не читается");
            let missing: Vec<_> = keys(&en).difference(&keys(&m)).cloned().collect();
            let extra: Vec<_> = keys(&m).difference(&keys(&en)).cloned().collect();
            assert!(missing.is_empty(), "{code}: нет ключей {missing:?}");
            assert!(extra.is_empty(), "{code}: лишние ключи {extra:?}");
            for (k, v) in &m {
                if k == "facts" {
                    let f = v.as_array().expect("facts — массив");
                    assert_eq!(f.len(), en["facts"].as_array().unwrap().len(), "{code}: число фактов");
                    for x in f {
                        let s = x.as_str().expect("факт — строка");
                        assert!(!s.trim().is_empty() && s.chars().count() <= 200, "{code}: факт {s}");
                    }
                    continue;
                }
                let s = v.as_str().unwrap_or_else(|| panic!("{code}.{k} — не строка"));
                assert!(!s.trim().is_empty(), "{code}.{k} пустой");
                assert_eq!(vars(s), vars(en[k].as_str().unwrap()), "{code}.{k}: подстановки");
            }
        }
    }

    #[test]
    fn fill_and_fallback() {
        assert_eq!(fill("{a} и {b}, {a}", &[("a", &1), ("b", &"x")]), "1 и x, 1");
        let m = merged("ja");
        assert_eq!(m["meta.name"].as_str(), Some("日本語"));
        assert_eq!(t("нет-такого-ключа"), "нет-такого-ключа");
    }
}
