// Команды для главного окна, появившиеся в S05: данные, обновление, настройки, лог.
// Как и в auth, вызывать их может только главное окно (не окно claude.ai).

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::auth::only_main;
use crate::scheduler::{self, Scheduler, Snapshot};
use crate::settings::{normalize_proxy, SettingsStore, INTERVALS};
use crate::applog;

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

#[derive(Serialize)]
pub struct SettingsView {
    pub interval_secs: u64,
    pub proxy: String,
    pub intervals: Vec<u64>,
}

fn view(app: &AppHandle) -> SettingsView {
    let s = app.state::<SettingsStore>().get();
    SettingsView {
        interval_secs: s.interval_secs,
        proxy: s.proxy.unwrap_or_default(),
        intervals: INTERVALS.to_vec(),
    }
}

#[tauri::command]
pub fn settings_get(window: WebviewWindow, app: AppHandle) -> CmdResult<SettingsView> {
    only_main(&window)?;
    Ok(view(&app))
}

/// Изменить интервал и/или прокси. Пустая строка прокси — выключить.
#[tauri::command]
pub fn settings_set(
    window: WebviewWindow,
    app: AppHandle,
    interval_secs: Option<u64>,
    proxy: Option<String>,
) -> CmdResult<SettingsView> {
    only_main(&window)?;
    if let Some(i) = interval_secs {
        if !INTERVALS.contains(&i) {
            return Err("Такой интервал не поддерживается".into());
        }
    }
    let proxy = match proxy {
        Some(p) => Some(normalize_proxy(&p).map_err(String::from)?),
        None => None,
    };
    let store = app.state::<SettingsStore>();
    let before = store.get();
    let after = store.update(|s| {
        if let Some(i) = interval_secs {
            s.interval_secs = i;
        }
        if let Some(p) = proxy {
            s.proxy = p;
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
    // Новый интервал или прокси — сразу проверяем, как работает
    if before != after {
        scheduler::kick(&app);
    }
    Ok(view(&app))
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
