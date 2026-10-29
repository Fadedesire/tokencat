// TokenCat — точка входа.
// S04: вход в claude.ai и хранение сессии. Клиент данных с планировщиком — S05,
// виды отображения — S07+.

// В релизной сборке на Windows не показываем консольное окно
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod auth;

fn main() {
    tauri::Builder::default()
        .manage(auth::Auth::new(Box::new(auth::store::KeyringStore)))
        .invoke_handler(tauri::generate_handler![
            auth::auth_status,
            auth::auth_login,
            auth::auth_logout,
            auth::auth_set_manual_key
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить TokenCat");
}
