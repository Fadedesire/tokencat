// Ключ сессии claude.ai (значение cookie `sessionKey`).
//
// Правила безопасности:
//  - тип SessionKey нельзя случайно напечатать: Debug/Display показывают только маску;
//  - значение достаётся явно через expose() и только для заголовка Cookie к claude.ai
//    и для записи в системное хранилище.
//
// Модуль не зависит от Tauri и сети — целиком покрыт юнит-тестами.

use std::fmt;

/// Префикс, с которого начинаются ключи сессии claude.ai.
pub const KEY_PREFIX: &str = "sk-ant-sid";
const MIN_LEN: usize = 24;
const MAX_LEN: usize = 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct SessionKey(String);

impl SessionKey {
    /// Ключ, прочитанный из своего webview или хранилища: только базовая проверка формата.
    pub fn from_trusted(raw: &str) -> Result<Self, KeyError> {
        normalize(raw).map(SessionKey)
    }

    /// Ключ, вставленный пользователем руками (раздел «Продвинутое»).
    pub fn from_user_input(raw: &str) -> Result<Self, KeyError> {
        normalize(raw).map(SessionKey)
    }

    /// Сырой секрет. Использовать только для запроса к claude.ai и для хранилища.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// «sk-ant-sid01…Xy9 (108 символов)» — безопасно показывать в UI.
    pub fn masked(&self) -> String {
        mask(&self.0)
    }
}

impl fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionKey({})", self.masked())
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.masked())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    WrongPrefix,
    TooShort,
    TooLong,
    BadChars,
}

impl KeyError {
    /// Текст для пользователя: что не так и что делать.
    pub fn user_message(&self) -> &'static str {
        match self {
            KeyError::Empty => "Поле пустое. Вставьте значение cookie sessionKey.",
            KeyError::WrongPrefix => "Это не похоже на ключ сессии: он начинается с sk-ant-sid.",
            KeyError::TooShort => "Ключ слишком короткий — похоже, скопирован не целиком.",
            KeyError::TooLong => "Ключ слишком длинный — похоже, вместе с ним скопировано лишнее.",
            KeyError::BadChars => "В ключе есть лишние символы. Скопируйте только значение sessionKey.",
        }
    }
}

/// Приводим вставленный текст к чистому ключу. Принимаем частые варианты копирования:
///   `sk-ant-sid01-…`, `"sk-ant-sid01-…"`, `sessionKey=sk-ant-sid01-…;`,
///   с пробелами и переводами строк по краям или посередине (перенос в DevTools).
pub fn normalize(raw: &str) -> Result<String, KeyError> {
    // Убираем все пробельные символы, включая неразрывные и переводы строк
    let mut s: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{200B}' && *c != '\u{FEFF}')
        .collect();
    if s.is_empty() {
        return Err(KeyError::Empty);
    }
    // Хвост «; Path=/…» или просто «;»
    if let Some(i) = s.find(';') {
        s.truncate(i);
    }
    // Префикс «sessionKey=» (без учёта регистра)
    if s.len() >= 11 && s[..11].eq_ignore_ascii_case("sessionKey=") {
        s.drain(..11);
    }
    // Кавычки и «ёлочки» по краям
    let s = s
        .trim_matches(|c| matches!(c, '"' | '\'' | '`' | '«' | '»' | '“' | '”'))
        .to_string();
    if s.is_empty() {
        return Err(KeyError::Empty);
    }
    if !s.starts_with(KEY_PREFIX) {
        return Err(KeyError::WrongPrefix);
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(KeyError::BadChars);
    }
    if s.len() < MIN_LEN {
        return Err(KeyError::TooShort);
    }
    if s.len() > MAX_LEN {
        return Err(KeyError::TooLong);
    }
    Ok(s)
}

/// Маска секрета: начало (префикс + 2 знака), последние 3 знака и длина.
pub fn mask(s: &str) -> String {
    let n = s.chars().count();
    if n <= 16 {
        return format!("••• ({n} символов)");
    }
    let head: String = s.chars().take(12).collect();
    let tail: String = s.chars().skip(n - 3).collect();
    format!("{head}…{tail} ({n} символов)")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "sk-ant-sid01-AbCdEfGhIjKlMnOpQrStUvWxYz_0123456789-abcdefXYZ";

    #[test]
    fn accepts_clean_key() {
        assert_eq!(normalize(GOOD).unwrap(), GOOD);
    }

    #[test]
    fn strips_spaces_quotes_and_newlines() {
        let messy = format!("  \"{}\"\r\n", GOOD);
        assert_eq!(normalize(&messy).unwrap(), GOOD);
        let split = format!("{}\n{}", &GOOD[..20], &GOOD[20..]);
        assert_eq!(normalize(&split).unwrap(), GOOD);
        let nbsp = format!("\u{00A0}{}\u{00A0}", GOOD);
        assert_eq!(normalize(&nbsp).unwrap(), GOOD);
        let ru_quotes = format!("«{}»", GOOD);
        assert_eq!(normalize(&ru_quotes).unwrap(), GOOD);
    }

    #[test]
    fn strips_cookie_syntax() {
        let c = format!("sessionKey={}; Path=/; Secure", GOOD);
        assert_eq!(normalize(&c).unwrap(), GOOD);
        let c2 = format!("SESSIONKEY={};", GOOD);
        assert_eq!(normalize(&c2).unwrap(), GOOD);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(normalize("   "), Err(KeyError::Empty));
        assert_eq!(normalize("\"\""), Err(KeyError::Empty));
        assert_eq!(normalize("hello world"), Err(KeyError::WrongPrefix));
        // Типичная ошибка при русской раскладке: «ыл-фте-ышв»
        assert_eq!(normalize("ыл-фте-ышв01-abc"), Err(KeyError::WrongPrefix));
        assert_eq!(normalize("sk-ant-sid01-x"), Err(KeyError::TooShort));
        let bad = format!("{}%2F", GOOD);
        assert_eq!(normalize(&bad), Err(KeyError::BadChars));
        let long = format!("{}{}", GOOD, "a".repeat(2000));
        assert_eq!(normalize(&long), Err(KeyError::TooLong));
    }

    #[test]
    fn mask_never_shows_secret() {
        let k = SessionKey::from_trusted(GOOD).unwrap();
        let m = k.masked();
        assert!(m.starts_with("sk-ant-sid01"));
        assert!(m.ends_with(&format!("({} символов)", GOOD.len())));
        assert!(!m.contains("AbCdEfGh"));
        assert!(!format!("{:?}", k).contains("AbCdEfGh"));
        assert!(!format!("{}", k).contains("AbCdEfGh"));
        assert_eq!(mask("short"), "••• (5 символов)");
    }
}
