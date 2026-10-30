// «Нажал — заработало» (B3 п. 4). Без Microsoft Edge WebView2 Tauri не может создать окно и
// программа молча закрывается: консоли у нас нет, пользователь не видит ничего. Установщик
// ставит WebView2 сам (webviewInstallMode), а portable-exe на старой Windows 10 — нет. Поэтому
// до запуска Tauri проверяем, есть ли WebView2, и если нет — показываем понятное окно со
// ссылкой на официальный установщик Microsoft.

/// Официальный «загрузчик» WebView2 от Microsoft (маленький exe, ставит без прав админа).
#[cfg(windows)]
const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// true — WebView2 есть, можно запускать. false — показали сообщение, программе надо выйти.
#[cfg(windows)]
pub fn ensure() -> bool {
    if tauri::webview_version().is_ok() {
        return true;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_YESNO,
    };
    // Настройки ещё не прочитаны — берём язык системы
    crate::i18n::set("system");
    let wide = |s: String| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let title = wide(crate::i18n::t("wv2.title"));
    let body = wide(crate::i18n::t("wv2.body"));
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND,
        )
    };
    if answer == IDYES {
        // Ссылку открывает браузер по умолчанию; explorer.exe не показывает консоль
        let _ = std::process::Command::new("explorer.exe").arg(WEBVIEW2_URL).spawn();
    }
    false
}

/// macOS: WKWebView входит в систему, проверять нечего.
#[cfg(not(windows))]
pub fn ensure() -> bool {
    true
}
