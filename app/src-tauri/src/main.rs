// TokenCat — точка входа.
// S04: вход в claude.ai и хранение сессии.
// S05: клиент данных (api, usage), планировщик, настройки сети, лог-файл.
// S07: вид «Поверх окон» (главное окно) + временное окно настроек.

// В релизной сборке на Windows не показываем консольное окно
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod applog;
mod auth;
mod commands;
mod overlay;
mod scheduler;
mod settings;
mod usage;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(auth::Auth::new(Box::new(auth::store::KeyringStore)))
        .manage(scheduler::Scheduler::new())
        .setup(|app| {
            let paths = app.path();
            if let Ok(dir) = paths.app_log_dir() {
                applog::init(dir);
            }
            let cfg = paths
                .app_config_dir()
                .map(|d| d.join("settings.json"))
                .unwrap_or_else(|_| std::env::temp_dir().join("tokencat-settings.json"));
            let store = settings::SettingsStore::load(cfg);
            let s = store.get();
            applog::info(format!(
                "TokenCat {} запущен; интервал {} с; прокси: {}",
                env!("CARGO_PKG_VERSION"),
                s.interval_secs,
                settings::proxy_for_log(s.proxy.as_deref())
            ));
            app.manage(store);
            tauri::async_runtime::spawn(scheduler::run(app.handle().clone()));
            overlay::setup(app.handle());
            Ok(())
        })
        .on_menu_event(|app, event| overlay::on_menu(app, event))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Moved(pos) = event {
                if window.label() == auth::MAIN_LABEL {
                    overlay::on_moved(window.app_handle(), *pos);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            auth::auth_status,
            auth::auth_login,
            auth::auth_logout,
            auth::auth_set_manual_key,
            commands::usage_snapshot,
            commands::usage_refresh,
            commands::settings_get,
            commands::settings_set,
            commands::open_log,
            overlay::overlay_fit,
            overlay::overlay_menu
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить TokenCat");
}
