// Автообновление (B4, п. 3).
//
// - Источник — GitHub Releases: файл latest.json последнего опубликованного релиза. Его и
//   подписи (.sig) делает релизный конвейер (ci/release.yml). Подпись проверяется публичным
//   ключом из tauri.conf.json: подменённый файл программа не установит.
// - Проверяем через 20 с после старта и дальше раз в сутки (счёт по часам компьютера, а не
//   по таймеру: после сна проверка не «съезжает»). Только основной профиль — дополнительные
//   копии программы проверяют лишь по кнопке, чтобы не ходить в сеть по нескольку раз.
// - Ненавязчиво: одно системное уведомление на версию, пункт «Обновить до X» в меню и строка
//   в настройках. Ставим только по клику пользователя.
// - Установленная программа (рядом с exe лежит uninstall.exe) обновляется сама: скачали →
//   тихий установщик NSIS → программа запускается заново. Portable-exe и dev-сборку не
//   трогаем — им предлагаем открыть страницу релиза.
// - Прокси из настроек действует и здесь.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::auth::emit_ui;
use crate::profile::Profile;
use crate::settings::SettingsStore;
use crate::{applog, i18n};

/// Страница релизов: сюда ведёт «Скачать» у portable-версии.
pub const RELEASES_URL: &str = "https://github.com/Fadedesire/tokencat/releases/latest";
/// Первая проверка после старта — не мешаем запуску и первому запросу лимитов
const FIRST_DELAY: Duration = Duration::from_secs(20);
/// Как часто просыпаемся, чтобы посмотреть, не прошли ли сутки
const TICK: Duration = Duration::from_secs(60 * 60);
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Что видит окно настроек. Секретов нет.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// idle | checking | none | available | downloading | installing | error
    pub state: &'static str,
    pub current: &'static str,
    /// Новая версия, если есть
    pub version: Option<String>,
    /// Что нового (текст релиза), коротко
    pub notes: Option<String>,
    /// 0–100 во время скачивания
    pub progress: Option<u8>,
    pub error: Option<String>,
    /// Может ли программа поставить обновление сама (false — portable / dev: «Скачать»)
    pub can_install: bool,
    pub checked_at_ms: Option<u64>,
}

struct Inner {
    status: Status,
    pending: Option<Update>,
}

static INNER: Mutex<Option<Inner>> = Mutex::new(None);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Установлена ли программа установщиком (тогда её можно обновить на месте).
pub fn can_install() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    #[cfg(windows)]
    {
        exe.parent().is_some_and(|d| d.join("uninstall.exe").exists())
    }
    #[cfg(target_os = "macos")]
    {
        exe.to_string_lossy().contains(".app/Contents/MacOS")
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = exe;
        false
    }
}

fn with<R>(f: impl FnOnce(&mut Inner) -> R) -> R {
    let mut g = INNER.lock().unwrap();
    let inner = g.get_or_insert_with(|| Inner {
        status: Status {
            state: "idle",
            current: env!("CARGO_PKG_VERSION"),
            can_install: can_install(),
            ..Default::default()
        },
        pending: None,
    });
    f(inner)
}

pub fn status() -> Status {
    with(|i| i.status.clone())
}

/// Поменять статус и сообщить окнам (и меню — там пункт «Обновить до X»).
fn set(app: &AppHandle, f: impl FnOnce(&mut Status)) {
    let (s, menu_changed) = with(|i| {
        let before = i.status.version.clone();
        f(&mut i.status);
        (i.status.clone(), before != i.status.version)
    });
    emit_ui(app, "update-changed", s);
    if menu_changed {
        crate::tray::rebuild_menu(app);
    }
}

/// Текст релиза коротко: без markdown-заголовков, не длиннее 400 символов.
pub fn short_notes(body: &str) -> Option<String> {
    let text: Vec<&str> = body
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .filter(|l| !l.is_empty())
        .collect();
    if text.is_empty() {
        return None;
    }
    let joined = text.join("\n");
    if joined.chars().count() > 400 {
        let mut t: String = joined.chars().take(399).collect();
        t.push('…');
        Some(t)
    } else {
        Some(joined)
    }
}

/// Пора ли автоматической проверке.
pub fn due(last_ms: Option<u64>, now: u64) -> bool {
    match last_ms {
        None => true,
        // Часы перевели назад — тоже проверяем
        Some(t) => now < t || now - t >= DAY_MS,
    }
}

/// Фоновая проверка: первый раз после старта, потом раз в сутки. Только основной профиль.
pub fn init(app: &AppHandle) {
    if !app.state::<Profile>().is_default() || cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        loop {
            let s = app.state::<SettingsStore>().get();
            if s.auto_update && due(s.update_checked_at_ms, now_ms()) {
                check(&app, false).await;
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Проверить обновления. manual — по кнопке (тогда «обновлений нет» тоже показываем).
pub async fn check(app: &AppHandle, manual: bool) {
    if matches!(status().state, "checking" | "downloading" | "installing") {
        return;
    }
    set(app, |s| {
        s.state = "checking";
        s.error = None;
    });
    let proxy = app.state::<SettingsStore>().get().proxy;
    let mut b = app.updater_builder().timeout(Duration::from_secs(30));
    if let Some(p) = proxy.as_deref().and_then(|p| tauri::Url::parse(p).ok()) {
        b = b.proxy(p);
    }
    let result = match b.build() {
        Ok(u) => u.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let now = now_ms();
    let _ = app.state::<SettingsStore>().update(|s| s.update_checked_at_ms = Some(now));
    match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            applog::info(format!("обновление: есть {version} (сейчас {})", env!("CARGO_PKG_VERSION")));
            let notes = update.body.as_deref().and_then(short_notes);
            with(|i| i.pending = Some(update));
            set(app, |s| {
                s.state = "available";
                s.version = Some(version.clone());
                s.notes = notes;
                s.checked_at_ms = Some(now);
            });
            notify_once(app, &version);
        }
        Ok(None) => {
            if manual {
                applog::info("обновление: версия последняя");
            }
            with(|i| i.pending = None);
            set(app, |s| {
                s.state = "none";
                s.version = None;
                s.notes = None;
                s.checked_at_ms = Some(now);
            });
        }
        Err(e) => {
            // Нет сети, GitHub недоступен — не страшно, проверим завтра. Пишем в лог.
            applog::warn(format!("обновление: проверка не удалась: {e}"));
            set(app, |s| {
                // Уже найденное обновление не теряем
                s.state = if s.version.is_some() { "available" } else { "error" };
                s.error = Some(i18n::t("upd.err_check"));
                s.checked_at_ms = Some(now);
            });
        }
    }
}

/// Одно уведомление на версию (номер запоминаем в settings.json).
fn notify_once(app: &AppHandle, version: &str) {
    let store = app.state::<SettingsStore>();
    let s = store.get();
    if s.update_notified.as_deref() == Some(version) {
        return;
    }
    let _ = store.update(|x| x.update_notified = Some(version.to_string()));
    if !s.notify {
        return;
    }
    crate::alerts::notify(
        app,
        i18n::tf("upd.notify_title", &[("v", &version)]),
        i18n::t("upd.notify_body"),
    );
}

/// «Обновить»: установленная программа — скачать и поставить; portable — открыть страницу.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if !status().can_install {
        open_url(RELEASES_URL);
        return Ok(());
    }
    let Some(update) = with(|i| i.pending.take()) else {
        return Err(i18n::t("upd.err_none"));
    };
    applog::info(format!("обновление: скачиваю {}", update.version));
    set(app, |s| {
        s.state = "downloading";
        s.progress = Some(0);
        s.error = None;
    });
    let mut got: u64 = 0;
    let mut last_pct: u8 = 0;
    let a = app.clone();
    let a2 = app.clone();
    let res = update
        .download_and_install(
            move |chunk, total| {
                got += chunk as u64;
                if let Some(t) = total.filter(|t| *t > 0) {
                    let pct = ((got * 100) / t).min(100) as u8;
                    if pct >= last_pct + 5 || pct == 100 {
                        last_pct = pct;
                        set(&a, |s| s.progress = Some(pct));
                    }
                }
            },
            move || {
                applog::info("обновление: скачано, запускаю установку");
                set(&a2, |s| s.state = "installing");
            },
        )
        .await;
    match res {
        Ok(()) => {
            // Windows: установщик уже запущен и сам закроет программу. macOS: .app заменён —
            // перезапускаемся.
            applog::info("обновление: установлено, перезапуск");
            app.restart();
        }
        Err(e) => {
            applog::warn(format!("обновление: не удалось установить: {e}"));
            with(|i| i.pending = Some(update));
            set(app, |s| {
                s.state = "error";
                s.progress = None;
                s.error = Some(i18n::t("upd.err_install"));
            });
            Err(i18n::t("upd.err_install"))
        }
    }
}

/// Открыть ссылку в браузере по умолчанию (без плагина: одна ссылка на релизы).
pub fn open_url(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer.exe").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        applog::warn(format!("не открыть ссылку: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_once_a_day() {
        assert!(due(None, 1000));
        assert!(!due(Some(1000), 1000 + DAY_MS - 1));
        assert!(due(Some(1000), 1000 + DAY_MS));
        // Часы перевели назад
        assert!(due(Some(5000), 1000));
    }

    #[test]
    fn notes_are_short_and_plain() {
        assert_eq!(short_notes("## Что нового\n\n- быстрее\n"), Some("Что нового\n- быстрее".into()));
        assert_eq!(short_notes("  \n "), None);
        let long = "я".repeat(1000);
        assert_eq!(short_notes(&long).unwrap().chars().count(), 400);
    }
}
