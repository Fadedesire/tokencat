// Лог-файл программы с ротацией (S05).
//
// Где: папка логов приложения (Windows: %LOCALAPPDATA%\app.tokencat.desktop\logs\tokencat.log).
// Ротация: файл больше 512 КБ → tokencat.1.log … tokencat.3.log, самый старый удаляется.
// Секретов в логе нет: пишем только свои короткие сообщения, а на всякий случай любой
// фрагмент, похожий на ключ `sk-ant-…`, перед записью заменяется маской (scrub).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FILE_NAME: &str = "tokencat.log";
const MAX_BYTES: u64 = 512 * 1024;
const KEEP: u32 = 3;

struct Log {
    dir: PathBuf,
    max: u64,
    file: Mutex<Option<File>>,
}

static LOG: OnceLock<Log> = OnceLock::new();

/// Вызвать один раз при старте. До этого (и в тестах) сообщения просто выводятся в консоль.
pub fn init(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    let _ = LOG.set(Log { dir, max: MAX_BYTES, file: Mutex::new(None) });
}

pub fn dir() -> Option<PathBuf> {
    LOG.get().map(|l| l.dir.clone())
}

pub fn info(msg: impl AsRef<str>) {
    write("INFO", msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    write("WARN", msg.as_ref());
}

fn write(level: &str, msg: &str) {
    let line = format!("{} {level} {}\n", timestamp(SystemTime::now()), scrub(msg));
    #[cfg(debug_assertions)]
    eprint!("{line}");
    if let Some(l) = LOG.get() {
        let _ = l.append(&line);
    }
}

impl Log {
    fn append(&self, line: &str) -> std::io::Result<()> {
        let mut g = self.file.lock().unwrap();
        let path = self.dir.join(FILE_NAME);
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size + line.len() as u64 > self.max {
            *g = None; // закрыть файл перед переименованием (на Windows иначе не даст)
            rotate(&self.dir, KEEP);
        }
        if g.is_none() {
            *g = Some(OpenOptions::new().create(true).append(true).open(&path)?);
        }
        g.as_mut().unwrap().write_all(line.as_bytes())
    }
}

/// tokencat.log → tokencat.1.log → … → tokencat.{keep}.log (удаляется).
fn rotate(dir: &Path, keep: u32) {
    let name = |i: u32| {
        if i == 0 {
            dir.join(FILE_NAME)
        } else {
            dir.join(format!("tokencat.{i}.log"))
        }
    };
    let _ = fs::remove_file(name(keep));
    for i in (0..keep).rev() {
        let _ = fs::rename(name(i), name(i + 1));
    }
}

/// Замена всего, что похоже на ключ сессии, маской.
pub fn scrub(s: &str) -> String {
    const P: &str = "sk-ant-";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(P) {
        out.push_str(&rest[..i]);
        out.push_str("sk-ant-***");
        let after = &rest[i + P.len()..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// «2026-10-29 14:05:09Z» без внешних библиотек (UTC).
pub fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Дни от 1970-01-01 → (год, месяц, день). Алгоритм Говарда Хиннанта.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn scrub_hides_keys() {
        assert_eq!(scrub("ключ sk-ant-sid01-AbC_d-9; ок"), "ключ sk-ant-***; ок");
        assert_eq!(scrub("a sk-ant-x b sk-ant-yy"), "a sk-ant-*** b sk-ant-***");
        assert_eq!(scrub("без секретов"), "без секретов");
        assert!(!scrub("Cookie: sessionKey=sk-ant-sid01-SECRET").contains("SECRET"));
    }

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01 00:00:00Z");
        // 2026-10-29 14:05:09 UTC
        let t = UNIX_EPOCH + Duration::from_secs(1_793_282_709);
        assert_eq!(timestamp(t), "2026-10-29 14:05:09Z");
        // Високосный день
        let t = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(timestamp(t), "2024-02-29 00:00:00Z");
    }

    #[test]
    fn rotation_keeps_three_old_files() {
        let dir = std::env::temp_dir().join(format!("tokencat-test-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = Log { dir: dir.clone(), max: 100, file: Mutex::new(None) };
        for i in 0..40 {
            log.append(&format!("строка номер {i:03}\n")).unwrap();
        }
        assert!(dir.join("tokencat.log").exists());
        assert!(dir.join("tokencat.1.log").exists());
        assert!(dir.join("tokencat.3.log").exists());
        assert!(!dir.join("tokencat.4.log").exists());
        assert!(fs::metadata(dir.join("tokencat.log")).unwrap().len() <= 100);
        // Последняя строка — в текущем файле
        drop(log);
        assert!(fs::read_to_string(dir.join("tokencat.log")).unwrap().contains("039"));
        let _ = fs::remove_dir_all(&dir);
    }
}
