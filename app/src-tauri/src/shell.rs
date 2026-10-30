// «Оболочка» вокруг панели (B1): окна настроек и онбординга, меню, профили, второй запуск.
//
// Правило Windows (S07): окна не создаём прямо в обработчике меню или значка — WebView2
// может зависнуть. Поэтому всё, что открывает окна, уходит в async-задачу.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::menu::MenuEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::auth::{emit_ui, Auth, MAIN_LABEL, ONBOARDING_LABEL, SETTINGS_LABEL};
use crate::profile::{self, Profile};
use crate::settings::SettingsStore;
use crate::{applog, autostart, commands, scheduler, tray};

/// Папки программы (общие для всех профилей; папки профиля считает profile.rs).
pub struct AppDirs {
    pub config_base: PathBuf,
}

pub fn known_profiles(app: &AppHandle) -> Vec<profile::Known> {
    profile::list(&app.state::<AppDirs>().config_base)
}

/// Настройки изменились: разослать окнам и пересобрать меню значка.
pub fn settings_changed(app: &AppHandle) {
    emit_ui(app, "settings-changed", commands::view(app));
    tray::rebuild_menu(app);
    // Котик и стиль строки меню видны на виджете и значке
    crate::views::refresh(app);
}

/// Сменился язык (B3): окна перезагрузят словарь, заголовки окон — заново.
pub fn language_changed(app: &AppHandle) {
    emit_ui(app, "lang-changed", crate::i18n::lang());
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = w.set_title(&window_title(app, &crate::i18n::t("win.settings")));
    }
    if let Some(w) = app.get_webview_window(ONBOARDING_LABEL) {
        let _ = w.set_title(&window_title(app, &crate::i18n::t("win.welcome")));
    }
}

/// Сменить вид (меню «Вид», настройки, онбординг).
pub fn set_view(app: &AppHandle, v: &str) -> Result<(), String> {
    if !crate::settings::VIEWS.contains(&v) {
        return Err(crate::i18n::t("err.view"));
    }
    let store = app.state::<SettingsStore>();
    if store.get().view == v {
        return Ok(());
    }
    store.update(|s| s.view = v.to_string())?;
    crate::views::apply(app);
    Ok(())
}

/// Онбординг уже пройден или пользователь давно вошёл (обновление со старой версии) —
/// второй раз не показываем.
pub fn migrate_onboarding(app: &AppHandle) {
    let store = app.state::<SettingsStore>();
    if store.get().onboarded {
        return;
    }
    if matches!(app.state::<Auth>().current_key(), Ok(Some(_))) {
        if let Err(e) = store.update(|s| s.onboarded = true) {
            applog::warn(e);
        }
    }
}

// ---------- Окна ----------

/// Показать панель (ЛКМ по значку, повторный запуск, пункт меню своего аккаунта).
/// Если онбординг не пройден — показать его.
pub fn show_panel(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !app.state::<SettingsStore>().get().onboarded {
            open_onboarding(&app);
            return;
        }
        // Виды B2: панель всплывает у виджета / значка
        if crate::views::is_popup(&app) {
            crate::views::show_popup(&app, None, false);
            return;
        }
        if let Some(w) = app.get_webview_window(MAIN_LABEL) {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    });
}

/// Новая копия этого же профиля постучалась (instance.rs) — показываем себя.
pub fn on_second_instance(app: &AppHandle) {
    applog::info("повторный запуск — показываю панель");
    show_panel(app);
}

fn focus(w: &WebviewWindow) {
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

/// Куда прокрутить только что открытое окно настроек (его забирает settings_goto)
static GOTO: Mutex<Option<String>> = Mutex::new(None);

pub fn take_goto() -> Option<String> {
    GOTO.lock().unwrap().take()
}

/// Окно настроек. `section` — куда прокрутить (например, "support").
pub fn open_settings(app: &AppHandle, section: Option<&str>) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        focus(&w);
        if let Some(sec) = section {
            let _ = tauri::Emitter::emit_to(app, SETTINGS_LABEL, "settings-goto", sec.to_string());
        }
        return;
    }
    *GOTO.lock().unwrap() = section.map(String::from);
    let r = with_browser_args(WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into())))
        .title(window_title(app, &crate::i18n::t("win.settings")))
        .inner_size(440.0, 640.0)
        .min_inner_size(400.0, 420.0)
        .resizable(true)
        .maximizable(false)
        .center()
        .build();
    if let Err(e) = r {
        applog::warn(format!("не удалось открыть настройки: {e}"));
    }
}

/// B3 (лёгкость): флаги WebView2 для ВСЕХ окон. Должны совпадать у окон с общей папкой
/// данных, иначе WebView2 не создаст окно — поэтому одна константа (и та же строка в
/// tauri.conf.json → additionalBrowserArgs у главного окна).
/// Первая часть — флаги Tauri по умолчанию; дальше: без GPU-процесса (панель крошечная,
/// программная отрисовка ей хватает), без фоновых сетевых служб и обновлений компонентов.
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-gpu --disable-background-networking --disable-component-update";

pub fn with_browser_args<'a, R: tauri::Runtime, M: Manager<R>>(
    b: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(windows)]
    {
        b.additional_browser_args(BROWSER_ARGS)
    }
    #[cfg(not(windows))]
    {
        b
    }
}

/// Онбординг: первый запуск или «Пройти заново» в настройках.
pub fn open_onboarding(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(ONBOARDING_LABEL) {
        focus(&w);
        return;
    }
    let r = with_browser_args(WebviewWindowBuilder::new(app, ONBOARDING_LABEL, WebviewUrl::App("onboarding.html".into())))
        .title(window_title(app, &crate::i18n::t("win.welcome")))
        .inner_size(560.0, 580.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .center()
        .build();
    if let Err(e) = r {
        applog::warn(format!("не удалось открыть онбординг: {e}"));
        // Без онбординга пользователь не должен остаться без панели
        let _ = app.state::<SettingsStore>().update(|s| s.onboarded = true);
        show_main(app);
    }
}

/// «TokenCat — настройки», у второго аккаунта — «TokenCat (Аня) — настройки».
fn window_title(app: &AppHandle, what: &str) -> String {
    let p = app.state::<Profile>();
    if p.is_default() {
        return format!("TokenCat — {what}");
    }
    let name = app.state::<SettingsStore>().get().account_name;
    format!("TokenCat ({}) — {what}", name.as_deref().unwrap_or(p.id()))
}

fn show_main(app: &AppHandle) {
    // Виды B2: панель появится по клику на виджет/значок — сам виджет уже на месте
    if crate::views::is_popup(app) {
        return;
    }
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        let _ = w.show();
    }
}

/// Окно онбординга закрыли (кнопкой «Готово» или крестиком): дальше — панель.
pub fn onboarding_closed(app: &AppHandle) {
    let store = app.state::<SettingsStore>();
    if !store.get().onboarded {
        applog::info("онбординг закрыт до конца — считаю пройденным");
        if let Err(e) = store.update(|s| s.onboarded = true) {
            applog::warn(e);
        }
        settings_changed(app);
    }
    show_main(app);
}

/// «Готово» в онбординге. Ошибка автозапуска онбординг не ломает: панель уже работает,
/// в лог пишем, а переключатель в настройках покажет, что автозапуск выключен.
pub fn finish_onboarding(app: &AppHandle, autostart_on: bool) -> Result<(), String> {
    let p = app.state::<Profile>().inner().clone();
    if let Err(e) = autostart::set(&p, autostart_on) {
        applog::warn(e);
    }
    app.state::<SettingsStore>().update(|s| s.onboarded = true)?;
    applog::info(format!("онбординг пройден; автозапуск: {}", if autostart_on { "да" } else { "нет" }));
    settings_changed(app);
    show_main(app);
    if let Some(w) = app.get_webview_window(ONBOARDING_LABEL) {
        let _ = w.destroy();
    }
    Ok(())
}

// ---------- Профили ----------

/// «Добавить аккаунт»: новый профиль и его копия программы.
pub fn add_account(app: &AppHandle) -> Result<(), String> {
    let known = known_profiles(app);
    // Уже есть дополнительная панель, в которой ещё не входили? Открываем её, а не плодим новую
    if let Some(k) = known.iter().find(|k| !k.profile.is_default() && k.account_name.is_none() && !k.signed_in_once) {
        applog::info(format!("открываю пустой профиль {}", k.profile.id()));
        return profile::launch(&k.profile);
    }
    let p = profile::next_free(&known)
        .ok_or_else(|| crate::i18n::tf("err.max_profiles", &[("n", &profile::MAX_PROFILES)]))?;
    let dir = p.config_dir(&app.state::<AppDirs>().config_base);
    std::fs::create_dir_all(&dir).map_err(|e| crate::i18n::tf("err.profile_create", &[("e", &e)]))?;
    // Пустой settings.json — чтобы профиль сразу появился в меню «Аккаунты»
    let file = dir.join("settings.json");
    if !file.exists() {
        std::fs::write(&file, "{}").map_err(|e| crate::i18n::tf("err.profile_create", &[("e", &e)]))?;
    }
    applog::info(format!("новый профиль {}", p.id()));
    profile::launch(&p)?;
    settings_changed(app);
    Ok(())
}

/// Открыть копию другого профиля (или показать себя).
fn open_profile(app: &AppHandle, id: &str) {
    let me = app.state::<Profile>().inner().clone();
    let Ok(p) = Profile::named(id) else { return };
    if p == me {
        show_panel(app);
        return;
    }
    if let Err(e) = profile::launch(&p) {
        applog::warn(e);
    }
}

/// «Удалить этот аккаунт из TokenCat» (только дополнительный профиль): выйти, убрать
/// автозапуск и папку профиля, закрыть эту копию.
pub fn remove_profile(app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    let p = app.state::<Profile>().inner().clone();
    if p.is_default() {
        return Err(crate::i18n::t("err.profile_main"));
    }
    crate::auth::sign_out(app, window)?;
    if let Err(e) = autostart::set(&p, false) {
        applog::warn(e);
    }
    let dir = p.config_dir(&app.state::<AppDirs>().config_base);
    std::fs::remove_dir_all(&dir).map_err(|e| crate::i18n::tf("err.profile_remove", &[("e", &e)]))?;
    applog::info(format!("профиль {} удалён", p.id()));
    app.exit(0);
    Ok(())
}

// ---------- Меню ----------

/// Нажатия в меню значка и в меню ПКМ панели (меню одно, см. tray.rs).
pub fn on_menu(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref().to_string();
    let app = app.clone();
    match id.as_str() {
        "refresh" => scheduler::kick(&app),
        "settings" => {
            tauri::async_runtime::spawn(async move { open_settings(&app, None) });
        }
        "support" => {
            tauri::async_runtime::spawn(async move { open_settings(&app, Some("support")) });
        }
        "on_top" => {
            let next = !app.state::<SettingsStore>().get().always_on_top;
            if let Err(e) = set_on_top(&app, next) {
                applog::warn(e);
            }
            settings_changed(&app);
        }
        "view_overlay" | "view_taskbar" | "view_menubar" => {
            let v = id.trim_start_matches("view_").to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = set_view(&app, &v) {
                    applog::warn(e);
                }
                // И галочки в меню, и окна настроек узнают о смене
                settings_changed(&app);
            });
        }
        "add_account" => {
            tauri::async_runtime::spawn(async move {
                if let Err(e) = add_account(&app) {
                    applog::warn(&e);
                    open_settings(&app, Some("account"));
                }
            });
        }
        "quit" => {
            applog::info("выход из меню");
            app.exit(0);
        }
        other => {
            if let Some(pid) = other.strip_prefix("profile:") {
                // Галочки «Аккаунтов» не меняются от клика — вернём как было
                tray::rebuild_menu(&app);
                let pid = pid.to_string();
                tauri::async_runtime::spawn(async move { open_profile(&app, &pid) });
            }
        }
    }
}

/// «Поверх окон»: сохранить и применить к панели.
pub fn set_on_top(app: &AppHandle, on: bool) -> Result<(), String> {
    app.state::<SettingsStore>().update(|s| s.always_on_top = on)?;
    applog::info(format!("поверх окон: {}", if on { "да" } else { "нет" }));
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        let _ = w.set_always_on_top(on);
    }
    Ok(())
}
