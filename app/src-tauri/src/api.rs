// HTTP-клиент claude.ai (S05): организации, /usage, прокси из настроек.
//
// Ключ уходит только в заголовке Cookie запроса к https://claude.ai и больше никуда.
// Запросы щадящие: один /usage за цикл планировщика, /organizations — только когда
// организация ещё неизвестна или сменилась.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::auth::key::SessionKey;
use crate::auth::verdict::{classify, ResponseInfo, Verdict};
use crate::settings::{clean_account_name, is_uuid_like};
use crate::usage::{self, Usage};

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
    fn new(verdict: Verdict, status: Option<u16>, detail: impl Into<String>) -> Self {
        Failure { verdict, status, detail: detail.into() }
    }
}

/// Сетевая ошибка (до ответа сервера) → вердикт.
/// Если задан прокси и не удалось подключиться — виноват, скорее всего, прокси.
pub fn transport_verdict(is_connect: bool, proxy_set: bool) -> Verdict {
    if proxy_set && is_connect {
        Verdict::Proxy
    } else {
        Verdict::Offline
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgInfo {
    pub uuid: String,
    pub name: Option<String>,
}

/// Сеть для одного цикла: клиент с прокси (или без).
pub struct Net {
    client: reqwest::Client,
    proxy_set: bool,
}

impl Net {
    pub fn new(proxy: Option<&str>) -> Result<Self, Failure> {
        let mut b = reqwest::Client::builder()
            .user_agent(UA)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            // Редиректы не нужны: claude.ai на /api отвечает напрямую, а редирект на /login
            // мы хотим видеть как ошибку, а не как HTML-страницу входа
            .redirect(reqwest::redirect::Policy::none());
        if let Some(p) = proxy {
            let px = reqwest::Proxy::all(p)
                .map_err(|_| Failure::new(Verdict::Proxy, None, "неверный адрес прокси"))?;
            b = b.proxy(px);
        }
        let client = b
            .build()
            .map_err(|e| Failure::new(Verdict::Offline, None, format!("клиент: {e}")))?;
        Ok(Net { client, proxy_set: proxy.is_some() })
    }

    async fn get_json(&self, key: &SessionKey, path: &str) -> Result<Value, Failure> {
        let net_err = |e: reqwest::Error| {
            let kind = if e.is_timeout() { "таймаут" } else if e.is_connect() { "подключение" } else { "сеть" };
            Failure::new(
                transport_verdict(e.is_connect(), self.proxy_set),
                None,
                format!("{kind} на {path}: {}", e.without_url()),
            )
        };
        let resp = self
            .client
            .get(format!("{BASE}{path}"))
            .header("Accept", "application/json")
            .header("Referer", format!("{BASE}/"))
            .header("anthropic-client-platform", "web_claude_ai")
            .header("Cookie", format!("sessionKey={}", key.expose()))
            .send()
            .await
            .map_err(net_err)?;

        let status = resp.status().as_u16();
        let cf_mitigated = resp.headers().contains_key("cf-mitigated");
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.bytes().await.map_err(net_err)?;
        let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY)]).to_string();

        let verdict = classify(&ResponseInfo { status, cf_mitigated, content_type: &content_type, body: &body });
        if verdict != Verdict::Ok {
            return Err(Failure::new(verdict, Some(status), format!("HTTP {status} на {path}")));
        }
        serde_json::from_str(&body)
            .map_err(|_| Failure::new(Verdict::Unexpected, Some(status), format!("не JSON на {path}")))
    }

    /// Проверить ключ и выбрать организацию (`preferred` — lastActiveOrg или прошлый запуск).
    pub async fn verify(&self, key: &SessionKey, preferred: Option<&str>) -> Result<OrgInfo, Failure> {
        let orgs = self.get_json(key, "/api/organizations").await?;
        pick_org(&orgs, preferred)
            .ok_or_else(|| Failure::new(Verdict::Unexpected, Some(200), "список организаций пуст"))
    }

    /// Имя аккаунта для шапки (B1, п. 9): `/api/account` → display_name, full_name…
    /// Почту не берём. None — имени нет (тогда покажем «TokenCat»).
    pub async fn account_name(&self, key: &SessionKey) -> Result<Option<String>, Failure> {
        let v = self.get_json(key, "/api/account").await?;
        Ok(pick_account_name(&v).map(|(name, _)| name))
    }

    pub async fn usage(&self, key: &SessionKey, org: &str) -> Result<Usage, Failure> {
        // uuid организации подставляем в путь — проверяем, что там нет ничего лишнего
        if !is_uuid_like(org) {
            return Err(Failure::new(Verdict::Unexpected, None, "странный uuid организации"));
        }
        let v = self.get_json(key, &format!("/api/organizations/{org}/usage")).await?;
        usage::parse(&v).map_err(|e| Failure::new(Verdict::Unexpected, Some(200), e.to_string()))
    }
}

/// Проверка ключа одним вызовом (окно входа, ручной ключ).
pub async fn verify(key: &SessionKey, preferred: Option<&str>, proxy: Option<&str>) -> Result<OrgInfo, Failure> {
    Net::new(proxy)?.verify(key, preferred).await
}

/// Выбор организации: сначала `preferred`, иначе первая с capability "chat", иначе первая.
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

/// Имя из ответа /api/account. Источник неофициальный, поэтому смотрим несколько мест:
/// сначала поля верхнего уровня, потом вложенный `account`. Возвращает имя и поле-источник
/// (для лога). Похожее на почту отбрасываем.
pub fn pick_account_name(v: &Value) -> Option<(String, &'static str)> {
    const FIELDS: [&str; 3] = ["display_name", "full_name", "name"];
    for root in [v, &v["account"]] {
        for f in FIELDS {
            if let Some(n) = root[f].as_str().and_then(clean_account_name) {
                return Some((n, f));
            }
        }
    }
    None
}

/// Запасное имя из названия организации: «Рома's Organization» → «Рома».
/// Название вида «почта's Organization» отбрасываем.
pub fn name_from_org(org: &str) -> Option<String> {
    let s = org.trim();
    let s = s
        .strip_suffix("'s Organization")
        .or_else(|| s.strip_suffix("’s Organization"))
        .unwrap_or(s);
    clean_account_name(s)
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
    fn account_names() {
        let v = json!({"uuid": "x", "email_address": "r@x.io", "full_name": "Роман П.", "display_name": "Рома"});
        assert_eq!(pick_account_name(&v), Some(("Рома".into(), "display_name")));
        let v = json!({"display_name": "", "full_name": "Роман"});
        assert_eq!(pick_account_name(&v).unwrap().0, "Роман");
        let v = json!({"account": {"display_name": "Рома"}});
        assert_eq!(pick_account_name(&v).unwrap().0, "Рома");
        // Почту как имя не показываем
        let v = json!({"display_name": "r@x.io", "email_address": "r@x.io"});
        assert_eq!(pick_account_name(&v), None);
        assert_eq!(pick_account_name(&json!([])), None);

        assert_eq!(name_from_org("Рома's Organization").as_deref(), Some("Рома"));
        assert_eq!(name_from_org("r@x.io's Organization"), None);
        assert_eq!(name_from_org("Студия Котиков").as_deref(), Some("Студия Котиков"));
    }

    #[test]
    fn transport_errors() {
        assert_eq!(transport_verdict(true, true), Verdict::Proxy);
        assert_eq!(transport_verdict(false, true), Verdict::Offline);
        assert_eq!(transport_verdict(true, false), Verdict::Offline);
    }

    #[test]
    fn client_builds_with_and_without_proxy() {
        // SOCKS требует фичу reqwest "socks" — тест ловит, если её забыли
        assert!(Net::new(Some("socks5://127.0.0.1:1080")).is_ok());
        assert!(Net::new(None).is_ok());
    }
}
