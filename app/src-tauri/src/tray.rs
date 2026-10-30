// Значок в трее / строке меню и общее меню программы (B1, п. 3).
//
// - Одно меню на всё: его показывает и значок в трее, и ПКМ по панели (меню собирается
//   заново при каждом изменении настроек, поэтому галочки всегда верные).
// - ЛКМ по значку — показать панель (если её закрыли другими окнами); в видах B2 —
//   открыть/закрыть всплывающую панель у значка (views.rs). Картинку значка рисует views.rs.
// - Подсказка значка — имя аккаунта и проценты: «TokenCat · Рома / 5 ч 42% · Неделя 18%».
//   У каждого профиля свой значок, подсказка помогает их различать.

use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::profile::{self, Profile};
use crate::scheduler::Snapshot;
use crate::settings::SettingsStore;
use crate::{applog, i18n, shell};

pub const TRAY_ID: &str = "tokencat";
/// Подсказка значка на Windows — не длиннее 127 символов
const TIP_MAX: usize = 120;

static LAST_TIP: Mutex<String> = Mutex::new(String::new());

/// Как подписать профиль в меню «Аккаунты».
fn profile_label(k: &profile::Known) -> String {
    match (&k.account_name, k.profile.is_default()) {
        (Some(n), _) => n.clone(),
        (None, true) => i18n::t("menu.main_account"),
        (None, false) => i18n::tf("menu.account", &[("id", &k.profile.id())]),
    }
}

/// Меню программы: трей и ПКМ по панели.
pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let s = app.state::<SettingsStore>().get();
    let me = app.state::<Profile>().inner().clone();

    let refresh = MenuItem::with_id(app, "refresh", i18n::t("menu.refresh"), true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", i18n::t("menu.settings"), true, None::<&str>)?;
    let popup = s.view != "overlay";
    // «Поверх окон» есть смысл менять только у плавающей панели
    let top = CheckMenuItem::with_id(app, "on_top", i18n::t("menu.on_top"), !popup, s.always_on_top, None::<&str>)?;

    // «Вид»: плавающая панель + вид своей ОС (B2)
    let v_overlay = CheckMenuItem::with_id(
        app, "view_overlay", i18n::t("menu.view_overlay"), true, s.view == "overlay", None::<&str>,
    )?;
    #[cfg(target_os = "macos")]
    let (os_id, os_name) = ("view_menubar", i18n::t("menu.view_menubar"));
    #[cfg(not(target_os = "macos"))]
    let (os_id, os_name) = ("view_taskbar", i18n::t("menu.view_taskbar"));
    let v_os = CheckMenuItem::with_id(app, os_id, os_name, true, popup, None::<&str>)?;
    let view = Submenu::with_items(app, i18n::t("menu.view"), true, &[&v_overlay, &v_os])?;

    // «Аккаунты»: все профили + «Добавить аккаунт»
    let known = shell::known_profiles(app);
    let mut acc_items: Vec<CheckMenuItem<tauri::Wry>> = Vec::new();
    for k in &known {
        acc_items.push(CheckMenuItem::with_id(
            app,
            format!("profile:{}", k.profile.id()),
            profile_label(k),
            true,
            k.profile == me,
            None::<&str>,
        )?);
    }
    let can_add = known.len() < profile::MAX_PROFILES;
    let add = MenuItem::with_id(app, "add_account", i18n::t("menu.add_account"), can_add, None::<&str>)?;
    let acc_sep = PredefinedMenuItem::separator(app)?;
    let accounts = Submenu::with_id(app, "accounts", i18n::t("menu.accounts"), true)?;
    for i in &acc_items {
        accounts.append(i)?;
    }
    accounts.append(&acc_sep)?;
    accounts.append(&add)?;

    let support = MenuItem::with_id(app, "support", i18n::t("menu.support"), true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", i18n::t("menu.quit"), true, None::<&str>)?;

    Menu::with_items(
        app,
        &[&refresh, &settings, &sep1, &top, &view, &accounts, &sep2, &support, &quit],
    )
}

/// Создать значок при старте.
pub fn setup(app: &AppHandle) {
    let menu = match build_menu(app) {
        Ok(m) => m,
        Err(e) => {
            applog::warn(format!("меню трея: {e}"));
            return;
        }
    };
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("TokenCat")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                // Плавающая панель — показать; всплывающая — открыть/закрыть у значка (B2)
                crate::views::on_tray_click(tray.app_handle(), Some(rect));
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    if let Err(e) = b.build(app) {
        applog::warn(format!("не удалось создать значок в трее: {e}"));
        return;
    }
    update(app, &app.state::<crate::scheduler::Scheduler>().snapshot());
}

/// Пересобрать меню значка (после смены настроек, имени аккаунта, списка профилей).
pub fn rebuild_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    match build_menu(app) {
        Ok(m) => {
            let _ = tray.set_menu(Some(m));
        }
        Err(e) => applog::warn(format!("меню трея: {e}")),
    }
}

/// Подсказка значка по снимку планировщика.
pub fn tooltip_text(snap: &Snapshot) -> String {
    let head = match &snap.account_name {
        Some(n) => format!("TokenCat · {n}"),
        None => "TokenCat".into(),
    };
    let pct = |l: &crate::usage::Limit| format!("{}%", l.percent.round() as i64);
    let body = match &snap.usage {
        Some(u) if snap.state != "signed_out" => {
            let mut parts = Vec::new();
            if let Some(l) = &u.session {
                parts.push(format!("{} {}", i18n::t("lim.session"), pct(l)));
            }
            if let Some(l) = &u.weekly {
                parts.push(format!("{} {}", i18n::t("lim.weekly"), pct(l)));
            }
            let mut t = parts.join(" · ");
            if snap.state != "ok" {
                t.push_str(&format!(" ({})", i18n::t("tip.stale")));
            }
            t
        }
        _ => snap.message.clone(),
    };
    let full = if body.is_empty() { head } else { format!("{head}\n{body}") };
    if full.chars().count() > TIP_MAX {
        let mut t: String = full.chars().take(TIP_MAX - 1).collect();
        t.push('…');
        t
    } else {
        full
    }
}

/// Обновить подсказку (вызывает планировщик). Не трогаем значок, если текст тот же.
pub fn update(app: &AppHandle, snap: &Snapshot) {
    // Значка может ещё не быть (первый цикл планировщика идёт параллельно со стартом)
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let tip = tooltip_text(snap);
    {
        let mut last = LAST_TIP.lock().unwrap();
        if *last == tip {
            return;
        }
        *last = tip.clone();
    }
    let _ = tray.set_tooltip(Some(tip));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Limit, Usage};

    fn lim(kind: &str, p: f64) -> Limit {
        Limit { kind: kind.into(), percent: p, resets_at: None, severity: None, active: false }
    }

    fn snap(state: &'static str, usage: Option<Usage>, name: Option<&str>) -> Snapshot {
        Snapshot {
            state,
            message: "Нет соединения".into(),
            usage,
            org_name: None,
            account_name: name.map(String::from),
            updated_at_ms: None,
            next_at_ms: None,
            fetching: false,
            show_log: false,
        }
    }

    #[test]
    fn tooltip_variants() {
        let u = Usage {
            session: Some(lim("session", 41.6)),
            weekly: Some(lim("weekly_all", 18.0)),
            others: vec![],
            breakdown: vec![],
            extra: None,
            source: "limits",
        };
        assert_eq!(tooltip_text(&snap("ok", Some(u.clone()), Some("Рома"))), "TokenCat · Рома\n5 ч 42% · Неделя 18%");
        assert_eq!(
            tooltip_text(&snap("offline", Some(u), None)),
            "TokenCat\n5 ч 42% · Неделя 18% (устарело)"
        );
        assert_eq!(tooltip_text(&snap("offline", None, None)), "TokenCat\nНет соединения");
        let long = "я".repeat(300);
        assert!(tooltip_text(&snap("ok", None, Some(&long))).chars().count() <= TIP_MAX);
    }
}
