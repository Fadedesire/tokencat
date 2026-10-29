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

use crate::api::{self, OrgInfo};
use crate::profile::Profile;
use crate::settings::SettingsStore;
use crate::{applog, scheduler};
use key::SessionKey;
use store::SecretStore;
use verdict::Verdict;

pub const MAIN_LABEL: &str = "main";
pub const LOGIN_LABEL: &str = "claude-login";
/// Окно настроек (B1). Права те же, что у главного.
pub const SETTINGS_LABEL: &str = "settings";
/// Онбординг первого запуска (B1)
pub const ONBOARDING_LABEL: &str = "onboarding";
/// Окна приложения, которым можно вызывать команды и получать события
pub const UI_LABELS: [&str; 3] = [MAIN_LABEL, SETTINGS_LABEL, ONBOARDING_LABEL];

/// Событие всем окнам приложения (но не окну claude.ai).
pub(crate) fn emit_ui<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    for l in UI_LABELS {
        if app.get_webview_window(l).is_some() {
            let _ = app.emit_to(l, event, payload.clone());
        }
    }
}
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

    pub fn org(&self) -> Option<OrgInfo> {
        self.org.lock().unwrap().clone()
    }

    pub fn set_org(&self, o: OrgInfo) {
        *self.org.lock().unwrap() = Some(o);
    }
}

// ---------- Что показываем главному окну ----------

/// Состояние входа для UI. Ключ сюда попадает только маской.
/// Проценты и ошибки сети приходят отдельно — от планировщика (событие `usage-updated`).
#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub masked: Option<String>,
    /// Ошибка чтения системного хранилища, если была
    pub error: Option<String>,
}

/// Состояние UI по вердикту запроса.
pub fn state_for(v: Verdict) -> &'static str {
    match v {
        Verdict::Ok => "ok",
        Verdict::SessionExpired => "expired",
        Verdict::Blocked => "blocked",
        Verdict::Offline | Verdict::Proxy => "offline",
        Verdict::RateLimited | Verdict::ServerError | Verdict::Unexpected => "error",
    }
}

fn proxy(app: &AppHandle) -> Option<String> {
    app.state::<SettingsStore>().get().proxy
}

/// Событие для главного окна. Секретов внутри нет.
#[derive(Clone, Serialize)]
struct AuthEvent {
    /// signed_in | signed_out | cancelled | problem
    kind: &'static str,
    message: Option<String>,
}

fn notify(app: &AppHandle, kind: &'static str, message: Option<String>) {
    emit_ui(app, "auth-changed", AuthEvent { kind, message });
}

/// Команды разрешены только окнам приложения (не окну claude.ai).
pub(crate) fn only_main(w: &WebviewWindow) -> CmdResult<()> {
    if UI_LABELS.contains(&w.label()) {
        Ok(())
    } else {
        Err("forbidden".into())
    }
}

// ---------- Команды ----------

/// Текущее состояние входа (без сети: только системное хранилище).
#[tauri::command]
pub async fn auth_status(window: WebviewWindow, app: AppHandle) -> CmdResult<AuthStatus> {
    only_main(&window)?;
    Ok(match app.state::<Auth>().current_key() {
        Ok(Some(k)) => AuthStatus { signed_in: true, masked: Some(k.masked()), error: None },
        Ok(None) => AuthStatus { signed_in: false, masked: None, error: None },
        Err(e) => AuthStatus { signed_in: false, masked: None, error: Some(e) },
    })
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
    let b = WebviewWindowBuilder::new(&app, LOGIN_LABEL, WebviewUrl::External(url))
        .title("Вход в claude.ai — TokenCat")
        .inner_size(480.0, 760.0)
        .center()
        // Только https: никаких file:, javascript: и прочего
        .on_navigation(|u| matches!(u.scheme(), "https" | "about" | "blob" | "data"));
    let b = separate_login_data(&app, b);
    b.build().map_err(err)?;

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

/// Дополнительный профиль (второй аккаунт) входит в claude.ai «с чистого листа», чтобы
/// не подхватить cookie основного аккаунта и не затереть их.
///   Windows: своя папка данных WebView2 для окна входа; перед входом её стираем, так что
///            старые cookie этого профиля тоже не мешают.
///   macOS:   «приватное» окно входа (данные только в памяти). Своё хранилище по профилю
///            WKWebView умеет лишь с macOS 14, а нам нужна 13.
/// Ключ после входа всё равно живёт в Keychain/Credential Manager, cookie окна нам больше не нужны.
fn separate_login_data<'a>(
    app: &AppHandle,
    b: WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let profile = app.state::<Profile>();
    if profile.is_default() {
        return b;
    }
    #[cfg(target_os = "macos")]
    {
        b.incognito(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        match app.path().app_local_data_dir().ok().and_then(|d| profile.webview_dir(&d)) {
            Some(dir) => {
                let _ = std::fs::remove_dir_all(&dir);
                b.data_directory(dir)
            }
            None => b.incognito(true),
        }
    }
}

/// После входа: имя аккаунта запросить заново (оно могло смениться вместе с аккаунтом).
fn after_sign_in(app: &AppHandle) {
    let r = app.state::<SettingsStore>().update(|s| {
        s.account_name_at = None;
        s.notify_level = 0;
    });
    if let Err(e) = r {
        applog::warn(e);
    }
}

/// После выхода: забыть имя аккаунта и уровень уведомлений.
fn after_sign_out(app: &AppHandle) {
    let r = app.state::<SettingsStore>().update(|s| {
        s.account_name = None;
        s.account_name_at = None;
        s.notify_level = 0;
    });
    if let Err(e) = r {
        applog::warn(e);
    }
    crate::shell::settings_changed(app);
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
        match api::verify(&k, org_hint.as_deref(), proxy(app).as_deref()).await {
            Ok(org) => {
                let auth = app.state::<Auth>();
                match auth.set_key(k, Some(org)) {
                    Ok(()) => {
                        let _ = w.destroy();
                        after_sign_in(app);
                        applog::info("вход выполнен");
                        notify(app, "signed_in", None);
                        scheduler::kick(app);
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
    sign_out(&app, &window)?;
    applog::info("выход из аккаунта");
    notify(&app, "signed_out", None);
    scheduler::kick(&app);
    Ok(())
}

/// Выйти без событий и лога (общая часть «Выйти» и «Удалить аккаунт»).
pub(crate) fn sign_out(app: &AppHandle, window: &WebviewWindow) -> CmdResult<()> {
    app.state::<Auth>().forget()?;
    if app.state::<Profile>().is_default() {
        // Данные webview общие для всех окон приложения: чистим через окно, из которого
        // нажали «Выйти». Так при следующем «Войти» claude.ai снова спросит логин.
        window.clear_all_browsing_data().map_err(err)?;
    } else if let Some(dir) = app
        .path()
        .app_local_data_dir()
        .ok()
        .and_then(|d| app.state::<Profile>().webview_dir(&d))
    {
        // У второго аккаунта своя папка окна входа — просто стираем её (если не занята)
        let _ = std::fs::remove_dir_all(dir);
    }
    after_sign_out(app);
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
    match api::verify(&k, None, proxy(&app).as_deref()).await {
        Ok(org) => {
            let masked = k.masked();
            app.state::<Auth>().set_key(k, Some(org))?;
            after_sign_in(&app);
            applog::info("ключ введён вручную и принят");
            notify(&app, "signed_in", None);
            scheduler::kick(&app);
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
        assert_eq!(state_for(Verdict::Ok), "ok");
        assert_eq!(state_for(Verdict::SessionExpired), "expired");
        assert_eq!(state_for(Verdict::Blocked), "blocked");
        assert_eq!(state_for(Verdict::Offline), "offline");
        assert_eq!(state_for(Verdict::Proxy), "offline");
        assert_eq!(state_for(Verdict::RateLimited), "error");
        assert_eq!(state_for(Verdict::Unexpected), "error");
    }

    #[test]
    fn status_never_contains_secret() {
        let k = SessionKey::from_trusted(GOOD).unwrap();
        let s = AuthStatus { signed_in: true, masked: Some(k.masked()), error: None };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("AbCdEfGh"));
    }
}
