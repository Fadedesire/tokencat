// Профили (B1, п. 9): несколько аккаунтов claude.ai = несколько копий программы.
//
// Каждая копия запускается со своим профилем (`--profile 2`) и живёт отдельно:
//   - ключ сессии — своя учётка в Credential Manager / Keychain (`tokencat-session-2`);
//   - settings.json — в папке `profiles\2\` рядом с основным;
//   - лог — в подпапке `logs\2\`;
//   - данные окна входа claude.ai (cookie) — своя папка WebView2 (Windows) или
//     «приватное» окно входа (macOS), чтобы вход во втором аккаунте не трогал первый;
//   - свой «один экземпляр»: повторный запуск того же профиля показывает уже открытую копию.
// Профиль по умолчанию (без `--profile`) лежит там же, где и раньше, — без миграции,
// чтобы уже вошедшие пользователи ничего не потеряли.

use std::path::{Path, PathBuf};

/// Имя учётки ключа в системном хранилище для профиля по умолчанию (как в S04)
pub const BASE_ACCOUNT: &str = "tokencat-session";
/// Сколько профилей вообще разрешаем (защита от случайного размножения копий)
pub const MAX_PROFILES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// None — профиль по умолчанию
    name: Option<String>,
}

/// Имя профиля: латиница в нижнем регистре, цифры, `-` и `_`, до 32 символов.
/// Оно попадает в пути и имя учётки — поэтому строго.
pub fn valid_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

impl Profile {
    pub fn default_profile() -> Self {
        Profile { name: None }
    }

    /// Профиль по имени. «default» и пустое — профиль по умолчанию; кривое имя — ошибка.
    pub fn named(raw: &str) -> Result<Self, String> {
        let s = raw.trim().to_ascii_lowercase();
        if s.is_empty() || s == "default" {
            return Ok(Self::default_profile());
        }
        if !valid_name(&s) {
            return Err(format!("неверное имя профиля: {raw:?}"));
        }
        Ok(Profile { name: Some(s) })
    }

    /// Профиль из аргументов командной строки: `--profile 2` или `--profile=2`.
    /// Кривое имя — профиль по умолчанию (лучше открыть основной, чем упасть).
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            let v = if a == "--profile" {
                it.next()
            } else {
                a.strip_prefix("--profile=").map(String::from)
            };
            if let Some(v) = v {
                return Self::named(&v).unwrap_or_else(|_| Self::default_profile());
            }
        }
        Self::default_profile()
    }

    pub fn is_default(&self) -> bool {
        self.name.is_none()
    }

    /// «default» или имя профиля — для лога, меню и UI.
    pub fn id(&self) -> &str {
        self.name.as_deref().unwrap_or("default")
    }

    /// Аргументы, с которыми запускать копию этого профиля (автозапуск, «Добавить аккаунт»).
    pub fn launch_args(&self) -> Vec<String> {
        match &self.name {
            None => vec![],
            Some(n) => vec!["--profile".into(), n.clone()],
        }
    }

    /// Учётка ключа в Credential Manager / Keychain.
    pub fn keyring_account(&self) -> String {
        match &self.name {
            None => BASE_ACCOUNT.into(),
            Some(n) => format!("{BASE_ACCOUNT}-{n}"),
        }
    }

    /// Папка настроек профиля внутри папки настроек приложения.
    pub fn config_dir(&self, base: &Path) -> PathBuf {
        match &self.name {
            None => base.to_path_buf(),
            Some(n) => base.join("profiles").join(n),
        }
    }

    /// Папка лога профиля.
    pub fn log_dir(&self, base: &Path) -> PathBuf {
        match &self.name {
            None => base.to_path_buf(),
            Some(n) => base.join(n),
        }
    }

    /// Своя папка данных окна входа (только не для профиля по умолчанию: тот, как и раньше,
    /// пользуется общей папкой WebView2).
    pub fn webview_dir(&self, local_data: &Path) -> Option<PathBuf> {
        self.name.as_ref().map(|n| local_data.join("profiles").join(n).join("webview"))
    }

    /// Имя записи автозапуска: у каждого профиля своя. Без пробелов и скобок — на macOS
    /// из него получается имя файла LaunchAgent.
    pub fn autostart_name(&self) -> String {
        match &self.name {
            None => "TokenCat".into(),
            Some(n) => format!("TokenCat-{n}"),
        }
    }

    /// Номер копии для сдвига панели по умолчанию: основной — 0, «2» — 1, «3» — 2…
    pub fn slot(&self) -> u32 {
        match &self.name {
            None => 0,
            Some(n) => n.parse::<u32>().map(|v| v.saturating_sub(1).clamp(1, 7)).unwrap_or(1),
        }
    }
}

/// Профиль из списка «Аккаунты» в меню.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub profile: Profile,
    /// Имя аккаунта claude.ai, сохранённое в его settings.json (если уже входили)
    pub account_name: Option<String>,
}

/// Все известные профили: основной + папки `profiles\*` с settings.json.
pub fn list(config_base: &Path) -> Vec<Known> {
    let mut out = vec![Known {
        profile: Profile::default_profile(),
        account_name: read_account_name(&config_base.join("settings.json")),
    }];
    let mut names: Vec<String> = std::fs::read_dir(config_base.join("profiles"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().join("settings.json").is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| valid_name(n))
                .collect()
        })
        .unwrap_or_default();
    // «2», «3», «10» — по числу, остальное — по алфавиту
    names.sort_by_key(|n| (n.parse::<u64>().unwrap_or(u64::MAX), n.clone()));
    for n in names {
        let p = Profile { name: Some(n) };
        let account_name = read_account_name(&p.config_dir(config_base).join("settings.json"));
        out.push(Known { profile: p, account_name });
    }
    out
}

fn read_account_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["account_name"].as_str().filter(|s| !s.is_empty()).map(String::from)
}

/// Имя для нового профиля: наименьшее свободное число начиная с 2.
pub fn next_free(existing: &[Known]) -> Option<Profile> {
    if existing.len() >= MAX_PROFILES {
        return None;
    }
    (2..100u32)
        .map(|n| Profile { name: Some(n.to_string()) })
        .find(|p| !existing.iter().any(|k| &k.profile == p))
}

/// Запустить копию программы для профиля. Если она уже открыта, новая копия просто
/// покажет её и закроется (см. instance.rs).
pub fn launch(p: &Profile) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("не найден exe: {e}"))?;
    std::process::Command::new(exe)
        .args(p.launch_args())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("не удалось запустить копию: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_args() {
        assert!(Profile::from_args(args(&["tokencat.exe"])).is_default());
        assert_eq!(Profile::from_args(args(&["x", "--profile", "2"])).id(), "2");
        assert_eq!(Profile::from_args(args(&["x", "--profile=Work"])).id(), "work");
        assert!(Profile::from_args(args(&["x", "--profile", "default"])).is_default());
        // Опасные и кривые имена — в профиль по умолчанию, а не в чужую папку
        assert!(Profile::from_args(args(&["x", "--profile", "../evil"])).is_default());
        assert!(Profile::from_args(args(&["x", "--profile", "a b"])).is_default());
        assert!(Profile::from_args(args(&["x", "--profile"])).is_default());
    }

    #[test]
    fn default_profile_keeps_old_places() {
        let p = Profile::default_profile();
        let base = Path::new("cfg");
        assert_eq!(p.keyring_account(), "tokencat-session");
        assert_eq!(p.config_dir(base), PathBuf::from("cfg"));
        assert_eq!(p.log_dir(base), PathBuf::from("cfg"));
        assert!(p.webview_dir(base).is_none());
        assert!(p.launch_args().is_empty());
        assert_eq!(p.slot(), 0);
    }

    #[test]
    fn named_profile_is_separate() {
        let p = Profile::named("2").unwrap();
        let base = Path::new("cfg");
        assert_eq!(p.keyring_account(), "tokencat-session-2");
        assert_eq!(p.config_dir(base), Path::new("cfg").join("profiles").join("2"));
        assert_eq!(p.log_dir(base), Path::new("cfg").join("2"));
        assert!(p.webview_dir(base).is_some());
        assert_eq!(p.launch_args(), args(&["--profile", "2"]));
        assert_eq!(p.autostart_name(), "TokenCat-2");
        assert_eq!(p.slot(), 1);
        assert_eq!(Profile::named("3").unwrap().slot(), 2);
        assert_eq!(Profile::named("work").unwrap().slot(), 1);
    }

    #[test]
    fn list_and_next_free() {
        let dir = std::env::temp_dir().join(format!("tokencat-test-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"account_name":"Рома"}"#).unwrap();
        for n in ["10", "2", "bad name"] {
            let d = dir.join("profiles").join(n);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        // Папка без settings.json — не профиль
        std::fs::create_dir_all(dir.join("profiles").join("3")).unwrap();

        let l = list(&dir);
        let ids: Vec<&str> = l.iter().map(|k| k.profile.id()).collect();
        assert_eq!(ids, ["default", "2", "10"]);
        assert_eq!(l[0].account_name.as_deref(), Some("Рома"));
        assert_eq!(next_free(&l).unwrap().id(), "3");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
