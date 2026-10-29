// Вход в claude.ai и хранение сессии (шаг S04).
//
// Как это работает:
//  1. «Войти» открывает отдельное окно https://claude.ai/login. Пользователь входит как обычно.
//     Окну claude.ai доступ к командам приложения закрыт (capabilities + проверка окна ниже).
//  2. Раз в секунду читаем cookie своего webview. Как только появился `sessionKey` —
//     проверяем его запросом /api/organizations. Ответ 200 — сохраняем ключ в системное
//     хранилище, закрываем окно и сообщаем главному окну событием `auth-changed`.
//  3. При старте ключ читается из хранилища — повторный вход не нужен.
//  4. «Выйти» удаляет ключ из хранилища и стирает cookie/данные webview.
//  5. Протухшая сессия — только 401 или 403 с JSON-ошибкой от API (см. verdict.rs).
//     Блок Cloudflare/региона сессией не считается: ключ сохраняем, просим проверить VPN.

pub mod key;
pub mod store;
pub mod verdict;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::api::{self, LimitLine, OrgInfo};
use key::SessionKey;
use store::SecretStore;
use verdict::Verdict;

pub const MAIN_LABEL: &str = "main";
pub const LOGIN_LABEL: &str = "claude-login";
/// Сколько ждём, пока пользователь войдёт, прежде чем перестать опрашивать cookie
const LOGIN_TIMEOUT: Duration = Duration::from_secs(20 * 60);

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Состояние входа, общее для всех команд.
pub struct Auth {
    store: Box<dyn SecretStore>,
    /// Ключ в памяти, чтобы не дёргать хранилище на каждый запрос
    key: Mutex<Option<SessionKey>>,
    org: Mutex<Option<OrgInfo>>,
    /// Хранилище уже прочитано при этом запуске
    loaded: AtomicBool,
    login_running: AtomicBool,
}

impl Auth {
    pub fn new(store: Box<dyn SecretStore>) -> Self {
        Auth {
            store,
            key: Mutex::new(None),
            org: Mutex::new(None),
            loaded: AtomicBool::new(false),
            login_running: AtomicBool::new(false),
        }
    }

    /// Текущий ключ: из памяти, а при первом обращении — из системного хранилища.
    pub fn current_key(&self) -> Result<Option<SessionKey>, String> {
        if !self.loaded.load(Ordering::SeqCst) {
            let k = self.store.load()?;
            *self.key.lock().unwrap() = k;
            self.loaded.store(true, Ordering::SeqCst);
        }
        Ok(self.key.lock().unwrap().clone())
    }

    /// Сохранить новый ключ (после входа или ручного ввода).
    pub fn set_key(&self, k: SessionKey, org: Option<OrgInfo>) -> Result<(), String> {
        self.store.save(&k)?;
        *self.key.lock().unwrap() = Some(k);
        *self.org.lock().unwrap() = org;
        self.loaded.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Забыть ключ везде (в памяти и в хранилище).
    pub fn forget(&self) -> Result<(), String> {
        *self.key.lock().unwrap() = None;
        *self.org.lock().unwrap() = None;
        self.loaded.store(true, Ordering::SeqCst);
        self.store.delete()
    }

    fn org(&self) -> Option<OrgInfo> {
        self.org.lock().unwrap().clone()
    }

    fn set_org(&self, o: OrgInfo) {
        *self.org.lock().unwrap() = Some(o);
    }
}

// ---------- Что показываем главному окну ----------

/// Состояние для UI. Ключ сюда попадает только маской.
#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    /// signed_out | signed_in | expired | blocked | offline | error
    pub state: &'static str,
    pub message: String,
    pub masked: Option<String>,
    pub org_name: Option<String>,
    pub limits: Vec<LimitLine>,
}

/// Состояние UI по вердикту запроса.
pub fn state_for(v: Verdict) -> &'static str {
    match v {
        Verdict::Ok => "signed_in",
        Verdict::SessionExpired => "expired",
        Verdict::Blocked => "blocked",
        Verdict::Offline => "offline",
        Verdict::RateLimited | Verdict::ServerError | Verdict::Unexpected => "error",
    }
}

fn signed_out() -> AuthStatus {
    AuthStatus {
        state: "signed_out",
        message: "Вы не вошли в claude.ai".into(),
        masked: None,
        org_name: None,
        limits: vec![],
    }
}

/// Событие для главного окна. Секретов внутри нет.
#[derive(Clone, Serialize)]
struct AuthEvent {
    /// signed_in | signed_out | cancelled | problem
    kind: &'static str,
    message: Option<String>,
}

fn notify(app: &AppHandle, kind: &'static str, message: Option<String>) {
    let _ = app.emit_to(MAIN_LABEL, "auth-changed", AuthEvent { kind, message });
}

/// Команды разрешены только главному окну приложения (не окну claude.ai).
fn only_main(w: &WebviewWindow) -> CmdResult<()> {
    if w.label() == MAIN_LABEL {
        Ok(())
    } else {
        Err("forbidden".into())
    }
}

// ---------- Команды ----------

/// Текущее состояние входа + свежие проценты (временный экран S04; в S05 — планировщик).
#[tauri::command]
pub async fn auth_status(window: WebviewWindow, app: AppHandle) -> CmdResult<AuthStatus> {
    only_main(&window)?;
    let auth = app.state::<Auth>();
    let key = match auth.current_key() {
        Ok(Some(k)) => k,
        Ok(None) => return Ok(signed_out()),
        Err(e) => {
            return Ok(AuthStatus {
                state: "error",
                message: e,
                ..signed_out()
            })
        }
    };
    let masked = Some(key.masked());

    let fail = |f: api::Failure, org_name: Option<String>| AuthStatus {
        state: state_for(f.verdict),
        message: f.verdict.user_message().to_string(),
        masked: masked.clone(),
        org_name,
        limits: vec![],
    };

    let org = match auth.org() {
        Some(o) => o,
        None => match api::verify(&key, None).await {
            Ok(o) => {
                auth.set_org(o.clone());
                o
            }
            Err(f) => return Ok(fail(f, None)),
        },
    };
    match api::usage_preview(&key, &org.uuid).await {
        Ok(limits) => Ok(AuthStatus {
            state: "signed_in",
            message: Verdict::Ok.user_message().into(),
            masked: masked.clone(),
            org_name: org.name,
            limits,
        }),
        Err(f) => Ok(fail(f, org.name)),
    }
}

/// Открыть окно входа claude.ai и ждать появления сессии.
#[tauri::command]
pub async fn auth_login(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        w.show().map_err(err)?;
        w.unminimize().ok();
        w.set_focus().map_err(err)?;
        return Ok(());
    }
    let url = Url::parse(&format!("{}/login", api::BASE)).map_err(err)?;
    WebviewWindowBuilder::new(&app, LOGIN_LABEL, WebviewUrl::External(url))
        .title("Вход в claude.ai — TokenCat")
        .inner_size(480.0, 760.0)
        .center()
        // Только https: никаких file:, javascript: и прочего
        .on_navigation(|u| matches!(u.scheme(), "https" | "about" | "blob" | "data"))
        .build()
        .map_err(err)?;

    let auth = app.state::<Auth>();
    if !auth.login_running.swap(true, Ordering::SeqCst) {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            watch_login(&app2).await;
            app2.state::<Auth>().login_running.store(false, Ordering::SeqCst);
        });
    }
    Ok(())
}

/// Cookie claude.ai из своего webview. На Windows вызов из основного потока может
/// зависнуть (WebView2) — поэтому всегда в отдельном потоке (проверено в S02).
async fn read_cookies(w: WebviewWindow) -> Result<(Option<String>, Option<String>), String> {
    let cookies = tauri::async_runtime::spawn_blocking(move || {
        w.cookies_for_url(Url::parse(&format!("{}/", api::BASE)).unwrap())
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let mut key = None;
    let mut org = None;
    for c in cookies {
        match c.name() {
            "sessionKey" => key = Some(c.value().to_string()),
            "lastActiveOrg" => org = Some(c.value().to_string()),
            _ => {}
        }
    }
    Ok((key, org))
}

/// Опрос окна входа, пока не появится рабочая сессия, окно не закроют или не выйдет время.
async fn watch_login(app: &AppHandle) {
    let started = Instant::now();
    // Ключ, который уже проверяли и который не подошёл (старая cookie после «сессия истекла»)
    let mut rejected: Option<SessionKey> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(w) = app.get_webview_window(LOGIN_LABEL) else {
            notify(app, "cancelled", None);
            return;
        };
        if started.elapsed() > LOGIN_TIMEOUT {
            let _ = w.destroy();
            notify(app, "cancelled", Some("Время на вход истекло, попробуйте ещё раз".into()));
            return;
        }
        let Ok((Some(raw), org_hint)) = read_cookies(w.clone()).await else {
            continue;
        };
        let Ok(k) = SessionKey::from_trusted(&raw) else {
            continue;
        };
        if rejected.as_ref() == Some(&k) {
            continue;
        }
        match api::verify(&k, org_hint.as_deref()).await {
            Ok(org) => {
                let auth = app.state::<Auth>();
                match auth.set_key(k, Some(org)) {
                    Ok(()) => {
                        let _ = w.destroy();
                        notify(app, "signed_in", None);
                    }
                    Err(e) => notify(app, "problem", Some(e)),
                }
                return;
            }
            Err(f) if f.verdict == Verdict::SessionExpired => {
                // Старая cookie: ждём, пока пользователь войдёт и cookie сменится
                rejected = Some(k);
            }
            Err(f) => {
                // VPN, сеть, сбой сервера — подскажем и попробуем снова через 5 с
                notify(app, "problem", Some(f.verdict.user_message().into()));
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
        }
    }
}

/// Выйти: удалить ключ из хранилища и стереть cookie/данные webview.
#[tauri::command]
pub async fn auth_logout(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        let _ = w.destroy();
    }
    app.state::<Auth>().forget()?;
    // Данные webview общие для всех окон приложения: чистим через главное окно.
    // Так при следующем «Войти» claude.ai снова спросит логин.
    window.clear_all_browsing_data().map_err(err)?;
    notify(&app, "signed_out", None);
    Ok(())
}

/// Запасной путь: ключ, вставленный руками в «Продвинутом».
#[tauri::command]
pub async fn auth_set_manual_key(
    window: WebviewWindow,
    app: AppHandle,
    value: String,
) -> CmdResult<String> {
    only_main(&window)?;
    let k = SessionKey::from_user_input(&value).map_err(|e| e.user_message().to_string())?;
    match api::verify(&k, None).await {
        Ok(org) => {
            let masked = k.masked();
            app.state::<Auth>().set_key(k, Some(org))?;
            notify(&app, "signed_in", None);
            Ok(masked)
        }
        Err(f) if f.verdict == Verdict::SessionExpired => {
            Err("claude.ai не принял этот ключ. Возможно, он устарел — скопируйте заново.".into())
        }
        Err(f) => Err(format!("{}. Ключ не сохранён.", f.verdict.user_message())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use store::MemoryStore;

    const GOOD: &str = "sk-ant-sid01-AbCdEfGhIjKlMnOpQrStUvWxYz_0123456789";

    #[test]
    fn key_survives_restart() {
        // Первый запуск: вход
        let shared = std::sync::Arc::new(MemoryStore::default());
        struct Shared(std::sync::Arc<MemoryStore>);
        impl SecretStore for Shared {
            fn load(&self) -> Result<Option<SessionKey>, String> { self.0.load() }
            fn save(&self, k: &SessionKey) -> Result<(), String> { self.0.save(k) }
            fn delete(&self) -> Result<(), String> { self.0.delete() }
        }
        let a1 = Auth::new(Box::new(Shared(shared.clone())));
        assert!(a1.current_key().unwrap().is_none());
        a1.set_key(SessionKey::from_trusted(GOOD).unwrap(), None).unwrap();
        drop(a1);

        // «Перезапуск»: новый Auth читает ключ из хранилища
        let a2 = Auth::new(Box::new(Shared(shared.clone())));
        assert_eq!(a2.current_key().unwrap().unwrap().expose(), GOOD);

        // Выход: ключа нет ни в памяти, ни в хранилище
        a2.forget().unwrap();
        assert!(a2.current_key().unwrap().is_none());
        let a3 = Auth::new(Box::new(Shared(shared)));
        assert!(a3.current_key().unwrap().is_none());
    }

    #[test]
    fn states_for_ui() {
        assert_eq!(state_for(Verdict::Ok), "signed_in");
        assert_eq!(state_for(Verdict::SessionExpired), "expired");
        assert_eq!(state_for(Verdict::Blocked), "blocked");
        assert_eq!(state_for(Verdict::Offline), "offline");
        assert_eq!(state_for(Verdict::RateLimited), "error");
    }

    #[test]
    fn status_never_contains_secret() {
        let k = SessionKey::from_trusted(GOOD).unwrap();
        let s = AuthStatus {
            state: "signed_in",
            message: String::new(),
            masked: Some(k.masked()),
            org_name: None,
            limits: vec![],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("AbCdEfGh"));
    }
}
