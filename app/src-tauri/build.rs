fn main() {
    // Свой манифест Windows (B2): то же, что у Tauri по умолчанию (Common Controls 6), плюс
    // раздел compatibility «Windows 8 / 8.1 / 10–11». Без него Windows считает программу
    // «старой» и не даёт дочерним окнам WS_EX_LAYERED — а виджету в панели задач нужен
    // прозрачный фон. Сам manifest.xml — только ASCII и без комментариев: загрузчик
    // Windows (SxS) к нему придирчив, ошибка 14001 «параллельная конфигурация неправильна».
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("tauri-build: не удалось подготовить сборку");
}
