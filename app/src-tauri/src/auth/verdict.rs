// Что значит ответ claude.ai для сессии: всё ок, сессия протухла, мешает Cloudflare/регион
// или что-то временное. Без сети и без Tauri — покрыто тестами.
//
// Главное правило (уроки прототипа, CONTEXT.md): 403 от Cloudflare почти всегда означает
// регион/VPN, а не плохой ключ. Поэтому «сессия истекла» — только когда ответ точно
// пришёл от самого claude.ai: 401, или 403 с JSON-ошибкой API.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// 2xx — сессия рабочая
    Ok,
    /// Ключ не принят claude.ai — нужен повторный вход
    SessionExpired,
    /// Ответил Cloudflare (регион, VPN, проверка браузера)
    Blocked,
    /// 429 — слишком часто, нужно подождать
    RateLimited,
    /// 5xx и прочее временное
    ServerError,
    /// Неожиданный ответ
    Unexpected,
    /// Нет сети / сервер недоступен (classify() это не возвращает, ставит HTTP-клиент)
    Offline,
    /// Не удалось подключиться к прокси из настроек (тоже ставит HTTP-клиент)
    Proxy,
}

/// Минимум сведений об ответе, нужный для решения.
pub struct ResponseInfo<'a> {
    pub status: u16,
    /// Есть заголовок `cf-mitigated` (Cloudflare сам говорит, что вмешался)
    pub cf_mitigated: bool,
    /// Значение Content-Type (может быть пустым)
    pub content_type: &'a str,
    /// Начало тела ответа (достаточно первых пары КБ)
    pub body: &'a str,
}

/// Явные признаки страницы Cloudflare.
pub fn looks_like_cloudflare(info: &ResponseInfo) -> bool {
    if info.cf_mitigated {
        return true;
    }
    let b = info.body;
    b.contains("Just a moment")
        || b.contains("cf-chl")
        || b.contains("challenge-platform")
        || b.contains("Attention Required")
        || b.contains("cf-error")
        || b.contains("cf_chl_opt")
        || b.contains("Cloudflare Ray ID")
}

fn is_json(info: &ResponseInfo) -> bool {
    info.content_type.contains("json") || {
        let t = info.body.trim_start();
        t.starts_with('{') || t.starts_with('[')
    }
}

pub fn classify(info: &ResponseInfo) -> Verdict {
    match info.status {
        200..=299 => {
            // Cloudflare иногда отдаёт страницу проверки с кодом 200
            if looks_like_cloudflare(info) && !is_json(info) {
                Verdict::Blocked
            } else {
                Verdict::Ok
            }
        }
        401 => {
            if looks_like_cloudflare(info) {
                Verdict::Blocked
            } else {
                Verdict::SessionExpired
            }
        }
        403 => {
            if looks_like_cloudflare(info) {
                Verdict::Blocked
            } else if is_json(info) {
                // JSON-ошибка от самого API: permission_error / account_session_invalid
                Verdict::SessionExpired
            } else {
                // HTML-страница без явных признаков — всё равно скорее сеть/регион
                Verdict::Blocked
            }
        }
        429 => {
            if looks_like_cloudflare(info) {
                Verdict::Blocked
            } else {
                Verdict::RateLimited
            }
        }
        500..=599 => {
            if looks_like_cloudflare(info) {
                Verdict::Blocked
            } else {
                Verdict::ServerError
            }
        }
        _ => Verdict::Unexpected,
    }
}

impl Verdict {
    /// Текст для пользователя (SPEC, «Состояния и ошибки»).
    pub fn user_message(self) -> &'static str {
        match self {
            Verdict::Ok => "Всё работает",
            Verdict::SessionExpired => "Нужно войти заново",
            Verdict::Blocked => "claude.ai недоступен из вашей сети. Проверьте VPN",
            Verdict::RateLimited => "claude.ai просит подождать, повторю позже",
            Verdict::ServerError => "claude.ai временно не отвечает, повторю позже",
            Verdict::Unexpected => "Не удалось прочитать данные",
            Verdict::Offline => "Нет соединения с claude.ai",
            Verdict::Proxy => "Прокси не отвечает. Проверьте адрес прокси в настройках",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r<'a>(status: u16, cf: bool, ct: &'a str, body: &'a str) -> ResponseInfo<'a> {
        ResponseInfo { status, cf_mitigated: cf, content_type: ct, body }
    }

    const CF_PAGE: &str = "<!DOCTYPE html><html><head><title>Just a moment...</title>";
    const API_403: &str = r#"{"type":"error","error":{"type":"permission_error","message":"Invalid authorization"}}"#;

    #[test]
    fn ok() {
        assert_eq!(classify(&r(200, false, "application/json", "[]")), Verdict::Ok);
    }

    #[test]
    fn session_expired_only_from_api() {
        assert_eq!(classify(&r(401, false, "application/json", "{}")), Verdict::SessionExpired);
        assert_eq!(classify(&r(401, false, "", "")), Verdict::SessionExpired);
        assert_eq!(classify(&r(403, false, "application/json", API_403)), Verdict::SessionExpired);
        // JSON без content-type тоже узнаём
        assert_eq!(classify(&r(403, false, "", API_403)), Verdict::SessionExpired);
    }

    #[test]
    fn cloudflare_is_not_expired_session() {
        assert_eq!(classify(&r(403, true, "text/html", "")), Verdict::Blocked);
        assert_eq!(classify(&r(403, false, "text/html", CF_PAGE)), Verdict::Blocked);
        assert_eq!(classify(&r(401, false, "text/html", CF_PAGE)), Verdict::Blocked);
        assert_eq!(classify(&r(503, false, "text/html", CF_PAGE)), Verdict::Blocked);
        assert_eq!(classify(&r(429, true, "text/html", "")), Verdict::Blocked);
        assert_eq!(classify(&r(200, false, "text/html", CF_PAGE)), Verdict::Blocked);
        // Непонятная HTML-страница с 403 — тоже сеть, а не сессия
        assert_eq!(classify(&r(403, false, "text/html", "<html>Forbidden</html>")), Verdict::Blocked);
    }

    #[test]
    fn temporary_errors() {
        assert_eq!(classify(&r(429, false, "application/json", "{}")), Verdict::RateLimited);
        assert_eq!(classify(&r(502, false, "text/plain", "Bad gateway")), Verdict::ServerError);
        assert_eq!(classify(&r(302, false, "", "")), Verdict::Unexpected);
    }
}
