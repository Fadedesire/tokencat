// Автозапуск с системой (B1, п. 4).
//   Windows — запись в HKCU\...\Run (без прав администратора);
//   macOS   — LaunchAgent в ~/Library/LaunchAgents.
// У каждого профиля своя запись с его `--profile`, поэтому после перезагрузки поднимаются
// все аккаунты, у которых автозапуск включён.
// Где правда: в самой системе (запись есть или нет), а не в settings.json — пользователь мог
// выключить автозапуск в Диспетчере задач.
//
// Используем crate auto-launch напрямую (его же внутри использует tauri-plugin-autostart):
// плагин умеет только одну запись на программу.

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

use crate::profile::Profile;

fn entry(p: &Profile) -> Result<AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| format!("не найден exe: {e}"))?;
    // Windows: путь в кавычках, иначе запись сломается на папке с пробелом в имени
    #[cfg(windows)]
    let path = format!("\"{}\"", exe.display());
    #[cfg(not(windows))]
    let path = exe.display().to_string();
    let args = p.launch_args();
    AutoLaunchBuilder::new()
        .set_app_name(&p.autostart_name())
        .set_app_path(&path)
        .set_args(args.as_slice())
        .build()
        .map_err(|e| format!("автозапуск: {e}"))
}

pub fn is_enabled(p: &Profile) -> bool {
    entry(p).and_then(|a| a.is_enabled().map_err(|e| e.to_string())).unwrap_or(false)
}

pub fn set(p: &Profile, on: bool) -> Result<(), String> {
    let a = entry(p)?;
    let r = if on { a.enable() } else { a.disable() };
    r.map_err(|e| format!("Не удалось {} автозапуск: {e}", if on { "включить" } else { "выключить" }))
}
