// Уведомления о 5-часовом лимите (B1, п. 5): 80 %, 100 % и сброс.
//
// - Каждое уведомление — один раз на окно лимита. Какой уровень уже показали, хранится в
//   settings.json (`notify_level`), поэтому перезапуск программы их не повторяет.
// - «Сброс» сообщаем, только если до этого предупреждали (80 %+), иначе было бы
//   уведомление каждые пять часов. Сбросом считаем падение ниже 50 % после 80 %+:
//   так не нужна точная граница окна (время сброса в ответе может «дрожать»).
// - Выключено в настройках — уровень всё равно отслеживаем, но молчим: если включить
//   обратно, не прилетит старое уведомление.
// Показ — системные уведомления через notify-rust (не плагин Tauri: плагин подменяет
// window.Notification во всех окнах, включая окно claude.ai).

use tauri::{AppHandle, Manager};

use crate::profile::Profile;
use crate::settings::SettingsStore;
use crate::usage::Usage;
use crate::{applog, i18n};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Reached80,
    Reached100,
    Reset,
}

/// Чистая логика: прошлый уровень (0/80/100) + текущий процент → новый уровень и что сказать.
pub fn step(level: u8, percent: f64) -> (u8, Option<Alert>) {
    let now = if percent >= 99.5 {
        100
    } else if percent >= 80.0 {
        80
    } else {
        0
    };
    if now > level {
        let a = if now == 100 { Alert::Reached100 } else { Alert::Reached80 };
        return (now, Some(a));
    }
    if level >= 80 && percent < 50.0 {
        return (0, Some(Alert::Reset));
    }
    (level, None)
}

fn texts(a: Alert, account: Option<&str>) -> (String, String) {
    let who = account.map(|n| format!(" · {n}")).unwrap_or_default();
    let k = match a {
        Alert::Reached80 => "alert.80",
        Alert::Reached100 => "alert.100",
        Alert::Reset => "alert.reset",
    };
    let title = i18n::t(&format!("{k}.title"));
    (format!("TokenCat{who}: {title}"), i18n::t(&format!("{k}.body")))
}

/// Вызывается планировщиком после каждого удачного обновления.
pub fn on_usage(app: &AppHandle, usage: &Usage) {
    let Some(session) = &usage.session else { return };
    let store = app.state::<SettingsStore>();
    let s = store.get();
    let (level, alert) = step(s.notify_level, session.percent);
    if level != s.notify_level {
        if let Err(e) = store.update(|x| x.notify_level = level) {
            applog::warn(e);
        }
    }
    let Some(a) = alert else { return };
    if !s.notify {
        return;
    }
    applog::info(format!("уведомление: {a:?}"));
    let (title, body) = texts(a, s.account_name.as_deref());
    show(app, title, body);
}

/// Уведомление не о лимитах (B4: «вышла новая версия»).
pub fn notify(app: &AppHandle, title: String, body: String) {
    show(app, title, body);
}

/// Пробное уведомление из настроек («Проверить уведомления»).
pub fn test(app: &AppHandle) {
    let name = app.state::<SettingsStore>().get().account_name;
    let who = name.map(|n| format!(" · {n}")).unwrap_or_default();
    show(app, format!("TokenCat{who}"), i18n::t("alert.test"));
}

#[cfg(any(windows, target_os = "macos"))]
fn show(app: &AppHandle, title: String, body: String) {
    let id = app.config().identifier.clone();
    let profile = app.state::<Profile>().id().to_string();
    // Показ может занять долю секунды (WinRT / NSUserNotification) — не в потоке UI
    tauri::async_runtime::spawn_blocking(move || {
        let mut n = notify_rust::Notification::new();
        n.summary(&title).body(&body).appname("TokenCat");
        #[cfg(windows)]
        {
            // Как в плагине Tauri: свой AppUserModelID только у установленной программы
            // (его регистрирует установщик). У dev-сборки уведомление придёт от PowerShell.
            if !running_from_target_dir() {
                n.app_id(&id);
            }
        }
        #[cfg(target_os = "macos")]
        {
            if !tauri::is_dev() {
                let _ = notify_rust::set_application(&id);
            }
        }
        if let Err(e) = n.show() {
            applog::warn(format!("уведомление не показано (профиль {profile}): {e}"));
        }
    });
}

#[cfg(not(any(windows, target_os = "macos")))]
fn show(_app: &AppHandle, _title: String, _body: String) {}

#[cfg(windows)]
fn running_from_target_dir() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|d| d.ends_with("target\\debug") || d.ends_with("target\\release"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_once_per_window() {
        assert_eq!(step(0, 10.0), (0, None));
        assert_eq!(step(0, 80.0), (80, Some(Alert::Reached80)));
        assert_eq!(step(80, 85.0), (80, None)); // второй раз не говорим
        assert_eq!(step(80, 100.0), (100, Some(Alert::Reached100)));
        assert_eq!(step(100, 100.0), (100, None));
        // Сразу 100 % (например, после сна) — одно уведомление, не два
        assert_eq!(step(0, 100.0), (100, Some(Alert::Reached100)));
    }

    #[test]
    fn reset_only_after_warning() {
        assert_eq!(step(100, 3.0), (0, Some(Alert::Reset)));
        assert_eq!(step(80, 0.0), (0, Some(Alert::Reset)));
        // Небольшое «дрожание» процента — не сброс
        assert_eq!(step(80, 78.0), (80, None));
        assert_eq!(step(80, 60.0), (80, None));
        // Без предупреждения сброс не сообщаем
        assert_eq!(step(0, 0.0), (0, None));
    }

    #[test]
    fn texts_have_account_name() {
        let (t, _) = texts(Alert::Reached80, Some("Рома"));
        assert!(t.contains("Рома") && t.contains("80"));
        let (t, _) = texts(Alert::Reset, None);
        assert!(t.starts_with("TokenCat:"));
    }
}
