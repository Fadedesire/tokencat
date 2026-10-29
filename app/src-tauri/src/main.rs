// TokenCat — точка входа.
// S04: вход в claude.ai и хранение сессии.
// S05: клиент данных (api, usage), планировщик, настройки сети, лог-файл.
// S07: вид «Поверх окон» (главное окно).
// B1: профили (несколько аккаунтов), один экземпляр на профиль, трей, автозапуск,
//     уведомления, окно настроек, онбординг.

// На Windows не показываем консольное окно ни в релизе, ни в dev-сборке (лог — в файле,
// «Открыть лог» в настройках). В тестах консоль нужна, чтобы видеть результат.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod alerts;
mod api;
mod applog;
mod auth;
mod autostart;
mod commands;
mod instance;
mod overlay;
mod profile;
mod scheduler;
mod settings;
mod shell;
mod tray;
mod usage;

use tauri::Manager;

use profile::Profile;

fn main() {
    // `--profile <имя>` — копия программы для другого аккаунта (profile.rs)
    let profile = Profile::from_args(std::env::args().skip(1));
    let keyring_account = profile.keyring_account();

    tauri::Builder::default()
        .manage(profile.clone())
        .manage(auth::Auth::new(Box::new(auth::store::KeyringStore::new(keyring_account))))
        .manage(scheduler::Scheduler::new())
        .setup(move |app| {
            let paths = app.path();
            let config_base = paths
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("tokencat"));
            let cfg_dir = profile.config_dir(&config_base);

            // Один экземпляр на профиль: этот профиль уже открыт — он покажет себя, мы выходим
            // (окно панели ещё скрыто, пользователь ничего не увидит).
            let listener = match instance::acquire(&cfg_dir, profile.id()) {
                instance::Start::AlreadyRunning => {
                    app.cleanup_before_exit();
                    std::process::exit(0);
                }
                instance::Start::First(l) => l,
            };

            if let Ok(dir) = paths.app_log_dir() {
                applog::init(profile.log_dir(&dir));
            }
            let store = settings::SettingsStore::load(cfg_dir.join("settings.json"));
            let s = store.get();
            applog::info(format!(
                "TokenCat {} запущен; профиль {}; интервал {} с; прокси: {}",
                env!("CARGO_PKG_VERSION"),
                profile.id(),
                s.interval_secs,
                settings::proxy_for_log(s.proxy.as_deref())
            ));
            app.manage(store);
            app.manage(shell::AppDirs { config_base });

            let handle = app.handle().clone();
            shell::migrate_onboarding(&handle);
            tauri::async_runtime::spawn(scheduler::run(handle.clone()));
            overlay::setup(&handle);
            tray::setup(&handle);
            if let Some(l) = listener {
                let h = handle.clone();
                instance::listen(l, profile.id().to_string(), move || shell::on_second_instance(&h));
            }
            if !app.state::<settings::SettingsStore>().get().onboarded {
                shell::open_onboarding(&handle);
            }
            Ok(())
        })
        .on_menu_event(|app, event| shell::on_menu(app, event))
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Moved(pos) if window.label() == auth::MAIN_LABEL => {
                overlay::on_moved(window.app_handle(), *pos);
            }
            tauri::WindowEvent::Destroyed if window.label() == auth::ONBOARDING_LABEL => {
                shell::onboarding_closed(window.app_handle());
            }
            _ => {}
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
            commands::settings_goto,
            commands::settings_open,
            commands::notify_test,
            commands::onboarding_finish,
            commands::onboarding_open,
            commands::profile_add,
            commands::profile_remove,
            commands::open_log,
            overlay::overlay_fit,
            overlay::overlay_menu
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить TokenCat");
}
