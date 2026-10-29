// TokenCat — точка входа.
// S03: только каркас, открывает пустое окно. Модули входа (S04), данных (S05)
// и видов (S07+) добавляются следующими шагами.

// В релизной сборке на Windows не показываем консольное окно
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("не удалось запустить TokenCat");
}
