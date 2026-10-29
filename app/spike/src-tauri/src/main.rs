// TokenCat — спайк S02.
// Задача: доказать, что вход без ручного копирования ключей работает.
//  1) открываем своё окно https://claude.ai/login, пользователь входит как обычно;
//  2) читаем cookie `sessionKey` из СВОЕГО webview (cookies_for_url, видит и HttpOnly);
//  3) запрашиваем /api/organizations и /usage двумя способами:
//     A — нативный HTTP-клиент (reqwest) с этой cookie,
//     B — fetch() внутри скрытого окна claude.ai (настоящий браузерный движок);
//  4) показываем результат и пишем отчёт spike-report.json (без секретов).
//
// Секрет нигде не логируется и не уходит никуда, кроме claude.ai.

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{
    AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

const LOGIN_LABEL: &str = "claude";
const BASE: &str = "https://claude.ai";
// Браузерный User-Agent (Edge на Windows) для нативного клиента
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn login_window(app: &AppHandle) -> CmdResult<WebviewWindow> {
    app.get_webview_window(LOGIN_LABEL)
        .ok_or_else(|| "Окно входа не открыто. Нажмите «Войти».".to_string())
}

/// Маскируем секрет: показываем только начало и длину.
fn mask(s: &str) -> String {
    let head: String = s.chars().take(12).collect();
    format!("{head}… ({} символов)", s.chars().count())
}

// ---------- Окно входа ----------

#[tauri::command]
async fn open_login(app: AppHandle) -> CmdResult<()> {
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        w.show().map_err(err)?;
        w.set_focus().map_err(err)?;
        return Ok(());
    }
    let url = Url::parse(&format!("{BASE}/login")).map_err(err)?;
    WebviewWindowBuilder::new(&app, LOGIN_LABEL, WebviewUrl::External(url))
        .title("Вход в claude.ai — TokenCat")
        .inner_size(480.0, 760.0)
        .build()
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn show_login(app: AppHandle, visible: bool) -> CmdResult<()> {
    let w = login_window(&app)?;
    if visible {
        w.show().map_err(err)?;
        w.set_focus().map_err(err)?;
    } else {
        w.hide().map_err(err)?;
    }
    Ok(())
}

/// Полный выход: чистим данные webview и закрываем окно.
#[tauri::command]
async fn reset_login(app: AppHandle) -> CmdResult<()> {
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        w.clear_all_browsing_data().map_err(err)?;
        w.destroy().map_err(err)?;
    }
    Ok(())
}

// ---------- Cookie ----------

struct Session {
    session_key: Option<String>,
    last_active_org: Option<String>,
    names: Vec<String>,
}

/// Читаем cookie claude.ai из своего webview.
/// На Windows вызов из синхронного контекста может зависнуть (WebView2),
/// поэтому всегда уходим в отдельный поток.
async fn read_session(app: &AppHandle) -> CmdResult<Session> {
    let w = login_window(app)?;
    let cookies = tauri::async_runtime::spawn_blocking(move || {
        w.cookies_for_url(Url::parse(&format!("{BASE}/")).unwrap())
    })
    .await
    .map_err(err)?
    .map_err(err)?;

    let mut s = Session {
        session_key: None,
        last_active_org: None,
        names: vec![],
    };
    for c in cookies {
        let name = c.name().to_string();
        match name.as_str() {
            "sessionKey" => s.session_key = Some(c.value().to_string()),
            "lastActiveOrg" => s.last_active_org = Some(c.value().to_string()),
            _ => {}
        }
        s.names.push(name);
    }
    s.names.sort();
    s.names.dedup();
    Ok(s)
}

#[derive(Serialize)]
struct SessionInfo {
    found: bool,
    masked: Option<String>,
    has_last_active_org: bool,
    cookie_names: Vec<String>,
    current_url: Option<String>,
}

#[tauri::command]
async fn check_session(app: AppHandle) -> CmdResult<SessionInfo> {
    let s = read_session(&app).await?;
    let current_url = login_window(&app)
        .ok()
        .and_then(|w| w.url().ok())
        .map(|u| format!("{}{}", u.host_str().unwrap_or(""), u.path()));
    Ok(SessionInfo {
        found: s.session_key.is_some(),
        masked: s.session_key.as_deref().map(mask),
        has_last_active_org: s.last_active_org.is_some(),
        cookie_names: s.names,
        current_url,
    })
}

// ---------- Общий формат результата ----------

#[derive(Serialize, Default)]
struct FetchReport {
    method: String,
    orgs_status: Option<u16>,
    org_count: Option<usize>,
    org_uuid: Option<String>,
    usage_status: Option<u16>,
    usage: Option<Value>,
    /// Начало ответа, если пришёл не JSON (страница Cloudflare и т.п.)
    body_snippet: Option<String>,
    cloudflare: bool,
    error: Option<String>,
    ms: u128,
}

/// Признаки того, что ответ отдал Cloudflare, а не claude.ai.
fn looks_like_cloudflare(status: u16, cf_mitigated: bool, body: &str) -> bool {
    (status == 403 || status == 503 || status == 429)
        && (cf_mitigated
            || body.contains("Just a moment")
            || body.contains("cf-chl")
            || body.contains("Attention Required")
            || body.contains("cf-error"))
}

/// Из списка организаций берём ту, где есть capability "chat".
fn pick_org(orgs: &Value, preferred: Option<&str>) -> Option<String> {
    let arr = orgs.as_array()?;
    if let Some(p) = preferred {
        if arr.iter().any(|o| o["uuid"].as_str() == Some(p)) {
            return Some(p.to_string());
        }
    }
    arr.iter()
        .find(|o| {
            o["capabilities"]
                .as_array()
                .map(|c| c.iter().any(|x| x.as_str() == Some("chat")))
                .unwrap_or(false)
        })
        .or_else(|| arr.first())
        .and_then(|o| o["uuid"].as_str().map(String::from))
}

fn snippet(s: &str) -> String {
    s.chars().take(300).collect()
}

// ---------- Способ A: нативный клиент ----------

#[tauri::command]
async fn fetch_native(app: AppHandle) -> CmdResult<FetchReport> {
    let s = read_session(&app).await?;
    let key = s
        .session_key
        .ok_or("Нет cookie sessionKey — сначала войдите.")?;
    let started = Instant::now();
    let mut rep = FetchReport {
        method: "native (reqwest)".into(),
        ..Default::default()
    };

    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(err)?;

    let get = |url: String| {
        client
            .get(url)
            .header("Accept", "application/json")
            .header("Referer", format!("{BASE}/"))
            .header("anthropic-client-platform", "web_claude_ai")
            .header("Cookie", format!("sessionKey={key}"))
            .send()
    };

    // 1) организации
    let resp = match get(format!("{BASE}/api/organizations")).await {
        Ok(r) => r,
        Err(e) => {
            rep.error = Some(format!("Сеть: {e}"));
            rep.ms = started.elapsed().as_millis();
            return Ok(rep);
        }
    };
    let status = resp.status().as_u16();
    let cf = resp.headers().contains_key("cf-mitigated");
    let body = resp.text().await.unwrap_or_default();
    rep.orgs_status = Some(status);
    let orgs: Value = match serde_json::from_str(&body) {
        Ok(v) if status == 200 => v,
        _ => {
            rep.cloudflare = looks_like_cloudflare(status, cf, &body);
            rep.body_snippet = Some(snippet(&body));
            rep.ms = started.elapsed().as_millis();
            return Ok(rep);
        }
    };
    rep.org_count = orgs.as_array().map(|a| a.len());
    let Some(org) = pick_org(&orgs, s.last_active_org.as_deref()) else {
        rep.error = Some("Список организаций пуст".into());
        rep.ms = started.elapsed().as_millis();
        return Ok(rep);
    };
    rep.org_uuid = Some(org.clone());

    // 2) лимиты
    match get(format!("{BASE}/api/organizations/{org}/usage")).await {
        Ok(r) => {
            let status = r.status().as_u16();
            let cf = r.headers().contains_key("cf-mitigated");
            let body = r.text().await.unwrap_or_default();
            rep.usage_status = Some(status);
            match serde_json::from_str::<Value>(&body) {
                Ok(v) if status == 200 => rep.usage = Some(v),
                _ => {
                    rep.cloudflare = looks_like_cloudflare(status, cf, &body);
                    rep.body_snippet = Some(snippet(&body));
                }
            }
        }
        Err(e) => rep.error = Some(format!("Сеть: {e}")),
    }
    rep.ms = started.elapsed().as_millis();
    Ok(rep)
}

// ---------- Способ B: fetch внутри окна claude.ai ----------

/// Выполнить JS в окне и получить результат строкой (JSON).
async fn eval_value(w: &WebviewWindow, js: &str) -> CmdResult<Value> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    w.eval_with_callback(js, move |s| {
        let _ = tx.send(s);
    })
    .map_err(err)?;
    let raw = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(5)))
        .await
        .map_err(err)?
        .map_err(|_| "Окно claude.ai не ответило".to_string())?;
    // Результат JSON.stringify(...) приходит ещё раз сериализованным в строку
    let v: Value = serde_json::from_str(&raw).map_err(err)?;
    Ok(match v {
        Value::String(inner) => serde_json::from_str(&inner).unwrap_or(Value::String(inner)),
        other => other,
    })
}

const WEBVIEW_FETCH_JS: &str = r#"
(function () {
  window.__tc = null;
  const H = { 'Accept': 'application/json', 'anthropic-client-platform': 'web_claude_ai' };
  const t0 = performance.now();
  (async () => {
    const out = { method: 'webview fetch', cloudflare: false };
    try {
      const r = await fetch('/api/organizations', { credentials: 'include', headers: H });
      out.orgs_status = r.status;
      const txt = await r.text();
      let orgs = null;
      try { orgs = JSON.parse(txt); } catch (e) {}
      if (r.status !== 200 || !Array.isArray(orgs)) {
        out.body_snippet = txt.slice(0, 300);
        out.cloudflare = /Just a moment|cf-chl|Attention Required|cf-error/.test(txt);
      } else {
        out.org_count = orgs.length;
        const m = document.cookie.match(/(?:^|; )lastActiveOrg=([^;]+)/);
        const pref = m && orgs.find(o => o.uuid === m[1]);
        const org = pref || orgs.find(o => (o.capabilities || []).includes('chat')) || orgs[0];
        if (org) {
          out.org_uuid = org.uuid;
          const u = await fetch('/api/organizations/' + org.uuid + '/usage', { credentials: 'include', headers: H });
          out.usage_status = u.status;
          const ut = await u.text();
          try { out.usage = JSON.parse(ut); } catch (e) { out.body_snippet = ut.slice(0, 300); }
          if (u.status !== 200) { out.usage = null; out.body_snippet = ut.slice(0, 300); }
        } else {
          out.error = 'Список организаций пуст';
        }
      }
    } catch (e) {
      out.error = String(e);
    }
    out.ms = Math.round(performance.now() - t0);
    window.__tc = out;
  })();
  return true;
})()
"#;

#[tauri::command]
async fn fetch_webview(app: AppHandle) -> CmdResult<Value> {
    let w = login_window(&app)?;
    let host = w.url().map_err(err)?.host_str().unwrap_or("").to_string();
    if host != "claude.ai" {
        return Err(format!(
            "Окно сейчас на {host}, а не на claude.ai — завершите вход."
        ));
    }
    eval_value(&w, WEBVIEW_FETCH_JS).await?;
    // ждём результат до 25 с
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let v = eval_value(&w, "JSON.stringify(window.__tc ?? null)").await?;
        if !v.is_null() {
            return Ok(v);
        }
    }
    Err("Нет ответа от fetch за 25 с".into())
}

// ---------- Отчёт ----------

/// Пишем отчёт рядом со спайком (app/spike/spike-report.json), чтобы Claude прочитал его.
/// Внутри только статусы и проценты, без cookie.
#[tauri::command]
async fn save_report(report: Value) -> CmdResult<String> {
    let path = std::env::current_dir()
        .map_err(err)?
        .join("..")
        .join("spike-report.json");
    let text = serde_json::to_string_pretty(&report).map_err(err)?;
    std::fs::write(&path, text).map_err(err)?;
    Ok(path.canonicalize().unwrap_or(path).display().to_string())
}

fn main() {
    tauri::Builder::default()
        .on_window_event(|window, event| {
            // Окно входа не закрываем, а прячем: в нём живёт сессия для способа B
            if window.label() == LOGIN_LABEL {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            open_login,
            show_login,
            reset_login,
            check_session,
            fetch_native,
            fetch_webview,
            save_report
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
