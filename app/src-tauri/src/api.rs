// Минимальный клиент claude.ai для S04: проверить сессию и показать проценты.
// Полноценный клиент данных с разбором всех окон, планировщиком и прокси — шаг S05.
//
// Ключ уходит только в заголовке Cookie запроса к https://claude.ai и больше никуда.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::auth::key::SessionKey;
use crate::auth::verdict::{classify, ResponseInfo, Verdict};

pub const BASE: &str = "https://claude.ai";
// Браузерный User-Agent (Edge на Windows) — с ним работал спайк S02
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";
/// Больше этого тело не читаем (защита от огромных ответов)
const MAX_BODY: usize = 512 * 1024;

/// Ошибка запроса: вердикт для логики + короткая подробность для лога (без секретов).
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub verdict: Verdict,
    pub status: Option<u16>,
    pub detail: String,
}

impl Failure {
    fn offline(e: impl std::fmt::Display) -> Self {
        Failure { verdict: Verdict::Offline, status: None, detail: e.to_string() }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgInfo {
    pub uuid: String,
    pub name: Option<String>,
}

/// Одна строка лимита для временного экрана S04.
#[derive(Debug, Clone, Serialize)]
pub struct LimitLine {
    pub label: String,
    pub percent: f64,
    pub resets_at: Option<String>,
}

fn client() -> Result<reqwest::Client, Failure> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        // Редиректы не нужны: claude.ai на /api отвечает напрямую, а редирект на /login
        // мы хотим видеть как ошибку, а не как HTML-страницу входа
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(Failure::offline)
}

async fn get_json(c: &reqwest::Client, key: &SessionKey, path: &str) -> Result<Value, Failure> {
    let resp = c
        .get(format!("{BASE}{path}"))
        .header("Accept", "application/json")
        .header("Referer", format!("{BASE}/"))
        .header("anthropic-client-platform", "web_claude_ai")
        .header("Cookie", format!("sessionKey={}", key.expose()))
        .send()
        .await
        .map_err(|e| Failure::offline(e.without_url()))?;

    let status = resp.status().as_u16();
    let cf_mitigated = resp.headers().contains_key("cf-mitigated");
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.bytes().await.map_err(|e| Failure::offline(e.without_url()))?;
    let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY)]).to_string();

    let verdict = classify(&ResponseInfo {
        status,
        cf_mitigated,
        content_type: &content_type,
        body: &body,
    });
    if verdict != Verdict::Ok {
        return Err(Failure {
            verdict,
            status: Some(status),
            detail: format!("HTTP {status} на {path}"),
        });
    }
    serde_json::from_str(&body).map_err(|_| Failure {
        verdict: Verdict::Unexpected,
        status: Some(status),
        detail: format!("не JSON на {path}"),
    })
}

/// Выбор организации: сначала `lastActiveOrg`, иначе первая с capability "chat", иначе первая.
pub fn pick_org(orgs: &Value, preferred: Option<&str>) -> Option<OrgInfo> {
    let arr = orgs.as_array()?;
    let has_chat = |o: &&Value| {
        o["capabilities"]
            .as_array()
            .map(|c| c.iter().any(|x| x.as_str() == Some("chat")))
            .unwrap_or(false)
    };
    let found = preferred
        .and_then(|p| arr.iter().find(|o| o["uuid"].as_str() == Some(p)))
        .or_else(|| arr.iter().find(has_chat))
        .or_else(|| arr.first())?;
    Some(OrgInfo {
        uuid: found["uuid"].as_str()?.to_string(),
        name: found["name"].as_str().map(String::from),
    })
}

/// Проверить, что ключ рабочий, и узнать организацию.
pub async fn verify(key: &SessionKey, preferred_org: Option<&str>) -> Result<OrgInfo, Failure> {
    let c = client()?;
    let orgs = get_json(&c, key, "/api/organizations").await?;
    pick_org(&orgs, preferred_org).ok_or(Failure {
        verdict: Verdict::Unexpected,
        status: Some(200),
        detail: "список организаций пуст".into(),
    })
}

/// Временный разбор /usage для экрана S04: только 5 ч и неделя.
pub fn parse_preview(usage: &Value) -> Vec<LimitLine> {
    let mut out = vec![];
    for (key, label) in [("five_hour", "5 часов"), ("seven_day", "Неделя")] {
        let w = &usage[key];
        if let Some(p) = w["utilization"].as_f64() {
            out.push(LimitLine {
                label: label.into(),
                percent: p,
                resets_at: w["resets_at"].as_str().map(String::from),
            });
        }
    }
    out
}

pub async fn usage_preview(key: &SessionKey, org: &str) -> Result<Vec<LimitLine>, Failure> {
    // uuid организации подставляем в путь — проверяем, что там нет ничего лишнего
    if org.is_empty() || !org.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(Failure {
            verdict: Verdict::Unexpected,
            status: None,
            detail: "странный uuid организации".into(),
        });
    }
    let c = client()?;
    let usage = get_json(&c, key, &format!("/api/organizations/{org}/usage")).await?;
    Ok(parse_preview(&usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_preferred_then_chat() {
        let orgs = json!([
            {"uuid": "aaa", "name": "API", "capabilities": ["api"]},
            {"uuid": "bbb", "name": "Рома", "capabilities": ["chat", "claude_pro"]}
        ]);
        assert_eq!(pick_org(&orgs, None).unwrap().uuid, "bbb");
        assert_eq!(pick_org(&orgs, Some("aaa")).unwrap().uuid, "aaa");
        assert_eq!(pick_org(&orgs, Some("zzz")).unwrap().uuid, "bbb");
        assert!(pick_org(&json!([]), None).is_none());
        assert!(pick_org(&json!({"error": 1}), None).is_none());
    }

    #[test]
    fn preview_survives_nulls_and_unknown_keys() {
        let u = json!({
            "five_hour": {"utilization": 67, "resets_at": "2026-10-29T15:00:00+00:00"},
            "seven_day": null,
            "something_new": {"x": 1}
        });
        let p = parse_preview(&u);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].percent, 67.0);
        assert!(parse_preview(&json!({})).is_empty());
    }
}
