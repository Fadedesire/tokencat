// Команды для окон приложения: данные, обновление, настройки, лог, онбординг, профили.
// Как и в auth, вызывать их могут только окна программы (не окно claude.ai).

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::auth::only_main;
use crate::profile::Profile;
use crate::scheduler::{self, Scheduler, Snapshot};
use crate::settings::{normalize_proxy, SettingsStore, INTERVALS, LANGS, VIEWS};
use crate::{alerts, applog, autostart, shell};

type CmdResult<T> = Result<T, String>;

/// Последний снимок данных (UI берёт его при загрузке, дальше слушает `usage-updated`).
#[tauri::command]
pub fn usage_snapshot(window: WebviewWindow, app: AppHandle) -> CmdResult<Snapshot> {
    only_main(&window)?;
    Ok(app.state::<Scheduler>().snapshot())
}

/// «Обновить сейчас».
#[tauri::command]
pub fn usage_refresh(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    scheduler::kick(&app);
    Ok(())
}

/// Всё, что нужно окнам о настройках. Секретов нет.
#[derive(Serialize, Clone)]
pub struct SettingsView {
    pub interval_secs: u64,
    pub intervals: Vec<u64>,
    pub proxy: String,
    pub view: String,
    pub views: Vec<&'static str>,
    pub always_on_top: bool,
    pub show_cat: bool,
    pub notify: bool,
    /// Автозапуск — как сейчас в системе
    pub autostart: bool,
    pub language: String,
    pub languages: Vec<&'static str>,
    pub onboarded: bool,
    pub account_name: Option<String>,
    pub profile_id: String,
    pub profile_default: bool,
    pub profiles_count: usize,
    pub version: &'static str,
    /// windows | macos
    pub os: &'static str,
}

pub fn view(app: &AppHandle) -> SettingsView {
    let s = app.state::<SettingsStore>().get();
    let p = app.state::<Profile>();
    SettingsView {
        interval_secs: s.interval_secs,
        intervals: INTERVALS.to_vec(),
        proxy: s.proxy.unwrap_or_default(),
        view: s.view,
        views: VIEWS.to_vec(),
        always_on_top: s.always_on_top,
        show_cat: s.show_cat,
        notify: s.notify,
        autostart: autostart::is_enabled(&p),
        language: s.language,
        languages: LANGS.to_vec(),
        onboarded: s.onboarded,
        account_name: s.account_name,
        profile_id: p.id().to_string(),
        profile_default: p.is_default(),
        profiles_count: shell::known_profiles(app).len(),
        version: env!("CARGO_PKG_VERSION"),
        os: if cfg!(target_os = "macos") { "macos" } else { "windows" },
    }
}

#[tauri::command]
pub fn settings_get(window: WebviewWindow, app: AppHandle) -> CmdResult<SettingsView> {
    only_main(&window)?;
    Ok(view(&app))
}

/// Куда прокрутить окно настроек при открытии (например, из «Поддержать проект»).
#[tauri::command]
pub fn settings_goto(window: WebviewWindow) -> CmdResult<Option<String>> {
    only_main(&window)?;
    Ok(shell::take_goto())
}

/// Изменение настроек из окна: только переданные поля. Всё применяется сразу.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SettingsPatch {
    pub interval_secs: Option<u64>,
    /// Пустая строка — выключить прокси
    pub proxy: Option<String>,
    pub view: Option<String>,
    pub always_on_top: Option<bool>,
    pub show_cat: Option<bool>,
    pub notify: Option<bool>,
    pub autostart: Option<bool>,
    pub language: Option<String>,
}

#[tauri::command]
pub fn settings_set(window: WebviewWindow, app: AppHandle, patch: SettingsPatch) -> CmdResult<SettingsView> {
    only_main(&window)?;
    if let Some(i) = patch.interval_secs {
        if !INTERVALS.contains(&i) {
            return Err("Такой интервал не поддерживается".into());
        }
    }
    if let Some(v) = &patch.view {
        if !VIEWS.contains(&v.as_str()) {
            return Err("Этот вид появится позже".into());
        }
    }
    if let Some(l) = &patch.language {
        if !LANGS.contains(&l.as_str()) {
            return Err("Такой язык не поддерживается".into());
        }
    }
    let proxy = match &patch.proxy {
        Some(p) => Some(normalize_proxy(p).map_err(String::from)?),
        None => None,
    };
    // Автозапуск живёт в системе, а не в settings.json
    if let Some(on) = patch.autostart {
        let p = app.state::<Profile>();
        autostart::set(&p, on)?;
        applog::info(format!("автозапуск: {}", if on { "да" } else { "нет" }));
    }
    if let Some(on) = patch.always_on_top {
        shell::set_on_top(&app, on)?;
    }

    let store = app.state::<SettingsStore>();
    let before = store.get();
    let after = store.update(|s| {
        if let Some(i) = patch.interval_secs {
            s.interval_secs = i;
        }
        if let Some(p) = proxy {
            s.proxy = p;
        }
        if let Some(v) = patch.view {
            s.view = v;
        }
        if let Some(b) = patch.show_cat {
            s.show_cat = b;
        }
        if let Some(b) = patch.notify {
            s.notify = b;
        }
        if let Some(l) = patch.language {
            s.language = l;
        }
    })?;
    if before.interval_secs != after.interval_secs {
        applog::info(format!("интервал обновления: {} с", after.interval_secs));
    }
    if before.proxy != after.proxy {
        applog::info(format!(
            "прокси: {}",
            crate::settings::proxy_for_log(after.proxy.as_deref())
        ));
    }
    if before.show_cat != after.show_cat || before.notify != after.notify || before.language != after.language {
        applog::info(format!(
            "котик: {}, уведомления: {}, язык: {}",
            after.show_cat, after.notify, after.language
        ));
    }
    // Новый интервал или прокси — сразу проверяем, как работает
    if before.interval_secs != after.interval_secs || before.proxy != after.proxy {
        scheduler::kick(&app);
    }
    shell::settings_changed(&app);
    Ok(view(&app))
}

/// Пробное уведомление.
#[tauri::command]
pub fn notify_test(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    alerts::test(&app);
    Ok(())
}

/// Открыть окно настроек (из панели или онбординга).
#[tauri::command]
pub async fn settings_open(window: WebviewWindow, app: AppHandle, section: Option<String>) -> CmdResult<()> {
    only_main(&window)?;
    shell::open_settings(&app, section.as_deref());
    Ok(())
}

/// «Готово» в онбординге.
#[tauri::command]
pub async fn onboarding_finish(window: WebviewWindow, app: AppHandle, autostart: bool) -> CmdResult<()> {
    only_main(&window)?;
    shell::finish_onboarding(&app, autostart)
}

/// «Пройти онбординг заново» в настройках.
#[tauri::command]
pub async fn onboarding_open(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    shell::open_onboarding(&app);
    Ok(())
}

/// «Добавить аккаунт».
#[tauri::command]
pub async fn profile_add(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    shell::add_account(&app)
}

/// «Удалить этот аккаунт из TokenCat» (только дополнительный профиль).
#[tauri::command]
pub async fn profile_remove(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    shell::remove_profile(&app, &window)
}

/// Открыть папку с логом в Проводнике / Finder.
#[tauri::command]
pub fn open_log(window: WebviewWindow) -> CmdResult<()> {
    only_main(&window)?;
    let dir = applog::dir().ok_or("Лог ещё не создан")?;
    #[cfg(target_os = "windows")]
    let prog = "explorer";
    #[cfg(target_os = "macos")]
    let prog = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let prog = "xdg-open";
    std::process::Command::new(prog)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть папку лога: {e}"))
}
