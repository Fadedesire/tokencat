// Настройки программы: данные (S05), панель (S07), окно настроек и онбординг (B1).
//
// Файл `settings.json` в папке настроек приложения
// (Windows: %APPDATA%\app.tokencat.desktop\, macOS: ~/Library/Application Support/…);
// у дополнительных профилей — в `profiles\<имя>\settings.json` там же (см. profile.rs).
// Секретов здесь нет: ключ сессии живёт только в системном хранилище.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Допустимые интервалы опроса, секунды (решение пользователя 2026-10-29: 30 с по умолчанию)
pub const INTERVALS: [u64; 3] = [30, 60, 120];
pub const DEFAULT_INTERVAL: u64 = 30;

/// Виды (B2): «Поверх окон» на обеих ОС + свой вид у каждой ОС:
/// «В панели задач» (Windows) и «В строке меню» (macOS).
#[cfg(target_os = "macos")]
pub const VIEWS: [&str; 2] = ["overlay", "menubar"];
#[cfg(not(target_os = "macos"))]
pub const VIEWS: [&str; 2] = ["overlay", "taskbar"];
/// Что показывать в строке меню macOS: 5 ч + неделя, только 5 ч, только значок, кольцо.
pub const MENUBAR_STYLES: [&str; 4] = ["both", "session", "icon", "ring"];
/// Языки интерфейса (переводы — B3). "system" — язык системы, иначе английский.
pub const LANGS: [&str; 10] = ["system", "ru", "en", "es", "pt-BR", "de", "fr", "ja", "ko", "zh-Hans"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub interval_secs: u64,
    /// http://host:port, https://…, socks5://…, socks5h://… или None
    pub proxy: Option<String>,
    /// uuid организации с прошлого запуска (как cookie lastActiveOrg)
    pub last_org: Option<String>,
    /// Вид «Поверх окон» (S07): левый верхний угол панели в физических пикселях
    /// виртуального экрана. None — поставить в угол основного монитора.
    pub overlay_x: Option<i32>,
    pub overlay_y: Option<i32>,
    /// Держать панель поверх остальных окон
    pub always_on_top: bool,
    /// Вид: overlay | taskbar | menubar (см. VIEWS)
    pub view: String,
    /// Строка меню macOS: both | session | icon | ring (см. MENUBAR_STYLES)
    pub menubar_style: String,
    /// Показывать котика
    pub show_cat: bool,
    /// Уведомления о 80 % / 100 % 5-часового лимита и о его сбросе
    pub notify: bool,
    /// Язык интерфейса (см. LANGS)
    pub language: String,
    /// Онбординг пройден (или пропущен) — больше не показываем
    pub onboarded: bool,
    /// Имя аккаунта claude.ai для шапки, трея и меню «Аккаунты». Почту сюда не пишем.
    pub account_name: Option<String>,
    /// Когда имя последний раз получено (секунды от 1970) — обновляем раз в сутки
    pub account_name_at: Option<u64>,
    /// О каком уровне 5-часового лимита уже сообщили: 0, 80 или 100 (чтобы после
    /// перезапуска не повторять уведомление)
    pub notify_level: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            interval_secs: DEFAULT_INTERVAL,
            proxy: None,
            last_org: None,
            overlay_x: None,
            overlay_y: None,
            always_on_top: true,
            view: "overlay".into(),
            menubar_style: "both".into(),
            show_cat: true,
            notify: true,
            language: "system".into(),
            onboarded: false,
            account_name: None,
            account_name_at: None,
            notify_level: 0,
        }
    }
}

impl Settings {
    /// Чиним то, что могли испортить руками в файле: странный интервал, кривой прокси.
    pub fn sanitized(mut self) -> Self {
        if !INTERVALS.contains(&self.interval_secs) {
            self.interval_secs = DEFAULT_INTERVAL;
        }
        self.proxy = self.proxy.as_deref().and_then(|p| normalize_proxy(p).ok().flatten());
        if !self.last_org.as_deref().is_some_and(is_uuid_like) {
            self.last_org = None;
        }
        // Позиция имеет смысл только целиком
        if self.overlay_x.is_none() || self.overlay_y.is_none() {
            self.overlay_x = None;
            self.overlay_y = None;
        }
        if !VIEWS.contains(&self.view.as_str()) {
            self.view = VIEWS[0].into();
        }
        if !MENUBAR_STYLES.contains(&self.menubar_style.as_str()) {
            self.menubar_style = MENUBAR_STYLES[0].into();
        }
        if !LANGS.contains(&self.language.as_str()) {
            self.language = "system".into();
        }
        if !matches!(self.notify_level, 0 | 80 | 100) {
            self.notify_level = 0;
        }
        self.account_name = self.account_name.as_deref().and_then(clean_account_name);
        self
    }
}

/// Имя аккаунта для показа: без почты, без пробелов по краям, не длиннее 64 символов
/// (обрезает с «…» уже интерфейс). Пустое или похожее на почту — None.
pub fn clean_account_name(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.contains('@') {
        return None;
    }
    Some(s.chars().take(64).collect())
}

/// uuid организации: только hex и дефисы (подставляется в путь запроса).
pub fn is_uuid_like(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Проверка и приведение адреса прокси. Пустая строка — прокси выключен (Ok(None)).
/// `127.0.0.1:10808` без схемы считаем http://.
pub fn normalize_proxy(raw: &str) -> Result<Option<String>, &'static str> {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty() {
        return Ok(None);
    }
    let (scheme, rest) = match s.split_once("://") {
        Some((sc, r)) => (sc.to_ascii_lowercase(), r),
        None => ("http".to_string(), s),
    };
    if !matches!(scheme.as_str(), "http" | "https" | "socks5" | "socks5h") {
        return Err("Прокси: поддерживаются адреса http://, https://, socks5:// и socks5h://");
    }
    if rest.contains('@') {
        // Пароль от прокси пришлось бы хранить в файле — пока не поддерживаем
        return Err("Прокси с логином и паролем пока не поддерживается");
    }
    if rest.contains(|c: char| c.is_whitespace() || c == '/' || c == '?' || c == '#') {
        return Err("Прокси: укажите только адрес и порт, например 127.0.0.1:10808");
    }
    let Some((host, port)) = rest.rsplit_once(':') else {
        return Err("Прокси: не указан порт, например 127.0.0.1:10808");
    };
    if host.is_empty() || port.parse::<u16>().map_or(true, |p| p == 0) {
        return Err("Прокси: неверный адрес или порт");
    }
    Ok(Some(format!("{scheme}://{host}:{port}")))
}

/// Безопасная для лога форма прокси (без логина, хотя мы его и не принимаем).
pub fn proxy_for_log(p: Option<&str>) -> String {
    match p {
        None => "нет".into(),
        Some(p) => p.rsplit('@').next().unwrap_or(p).to_string(),
    }
}

/// Настройки в памяти + файл на диске.
pub struct SettingsStore {
    path: PathBuf,
    cur: Mutex<Settings>,
}

impl SettingsStore {
    /// Читает файл. Нет файла или он испорчен — настройки по умолчанию (не падаем).
    pub fn load(path: PathBuf) -> Self {
        let cur = read_file(&path).unwrap_or_default().sanitized();
        SettingsStore { path, cur: Mutex::new(cur) }
    }

    pub fn get(&self) -> Settings {
        self.cur.lock().unwrap().clone()
    }

    /// Изменить и сохранить. Возвращает новые настройки.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut g = self.cur.lock().unwrap();
        let mut next = g.clone();
        f(&mut next);
        let next = next.sanitized();
        if next != *g {
            write_file(&self.path, &next)?;
            *g = next;
        }
        Ok(g.clone())
    }
}

fn read_file(path: &Path) -> Option<Settings> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Запись через временный файл, чтобы при сбое не остался обрезанный settings.json.
fn write_file(path: &Path, s: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Не удалось создать папку настроек: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| format!("Не удалось сохранить настройки: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Не удалось сохранить настройки: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_variants() {
        assert_eq!(normalize_proxy("").unwrap(), None);
        assert_eq!(normalize_proxy("   ").unwrap(), None);
        assert_eq!(normalize_proxy("127.0.0.1:10808").unwrap().unwrap(), "http://127.0.0.1:10808");
        assert_eq!(
            normalize_proxy(" SOCKS5://127.0.0.1:1080/ ").unwrap().unwrap(),
            "socks5://127.0.0.1:1080"
        );
        assert_eq!(normalize_proxy("socks5h://proxy.local:9050").unwrap().unwrap(), "socks5h://proxy.local:9050");
        assert!(normalize_proxy("ftp://1.2.3.4:21").is_err());
        assert!(normalize_proxy("127.0.0.1").is_err());
        assert!(normalize_proxy("127.0.0.1:99999").is_err());
        assert!(normalize_proxy("127.0.0.1:0").is_err());
        assert!(normalize_proxy("http://user:pass@1.2.3.4:8080").is_err());
        assert!(normalize_proxy("http://1.2.3.4:8080/path").is_err());
        assert!(normalize_proxy(":8080").is_err());
    }

    #[test]
    fn sanitize_fixes_bad_values() {
        let s = Settings {
            interval_secs: 7,
            proxy: Some("не прокси".into()),
            last_org: Some("../../etc".into()),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s, Settings::default());
        let ok = Settings {
            interval_secs: 120,
            proxy: Some("127.0.0.1:2080".into()),
            last_org: Some("58a28114-c56e-4a29-af8e-c95d508f8ad3".into()),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(ok.interval_secs, 120);
        assert_eq!(ok.proxy.as_deref(), Some("http://127.0.0.1:2080"));
        assert!(ok.last_org.is_some());
    }

    #[test]
    fn file_roundtrip_and_garbage() {
        let dir = std::env::temp_dir().join(format!("tokencat-test-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");

        // Файла нет — значения по умолчанию
        let st = SettingsStore::load(path.clone());
        assert_eq!(st.get(), Settings::default());

        st.update(|s| {
            s.interval_secs = 120;
            s.proxy = Some("socks5://127.0.0.1:1080".into());
        })
        .unwrap();
        // «Перезапуск»
        let st2 = SettingsStore::load(path.clone());
        assert_eq!(st2.get().interval_secs, 120);
        assert_eq!(st2.get().proxy.as_deref(), Some("socks5://127.0.0.1:1080"));

        // Испорченный файл и незнакомые поля не роняют программу
        std::fs::write(&path, "{ это не json").unwrap();
        assert_eq!(SettingsStore::load(path.clone()).get(), Settings::default());
        std::fs::write(&path, r#"{"interval_secs": 60, "future_field": true}"#).unwrap();
        assert_eq!(SettingsStore::load(path.clone()).get().interval_secs, 60);
        // Старое значение 5 мин больше не поддерживается — возвращаемся к 30 с
        std::fs::write(&path, r#"{"interval_secs": 300}"#).unwrap();
        assert_eq!(SettingsStore::load(path.clone()).get().interval_secs, 30);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_fields_defaults_and_cleanup() {
        // Старый settings.json (до B1) — новые поля получают значения по умолчанию
        let s: Settings = serde_json::from_str(r#"{"interval_secs": 60, "always_on_top": false}"#).unwrap();
        let s = s.sanitized();
        assert_eq!(s.interval_secs, 60);
        assert!(!s.always_on_top);
        assert_eq!(s.view, "overlay");
        assert!(s.show_cat && s.notify && !s.onboarded);
        assert_eq!(s.language, "system");

        let bad = Settings {
            view: "taskbar-из-будущего".into(),
            language: "klingon".into(),
            notify_level: 55,
            menubar_style: "neon".into(),
            account_name: Some("  roma@example.com ".into()),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(bad.view, "overlay");
        assert_eq!(bad.menubar_style, "both");
        assert_eq!(bad.language, "system");
        // Вид чужой ОС (файл перенесли с Mac на Windows) — тоже «Поверх окон»
        let foreign = if cfg!(target_os = "macos") { "taskbar" } else { "menubar" };
        let moved = Settings { view: foreign.into(), ..Settings::default() }.sanitized();
        assert_eq!(moved.view, "overlay");
        assert_eq!(bad.notify_level, 0);
        assert_eq!(bad.account_name, None);
        assert_eq!(clean_account_name("  Рома  ").as_deref(), Some("Рома"));
        assert_eq!(clean_account_name(&"я".repeat(100)).unwrap().chars().count(), 64);
    }
}
