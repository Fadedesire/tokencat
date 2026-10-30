// Кеш последних цифр на диске (B3, лёгкость: «до окна с цифрами ≤ 1 с»).
//
// После каждого удачного цикла планировщика сохраняем Usage в `last-usage.json` в папке
// профиля (только проценты и время сброса — без ключей и имён). При старте программа сразу
// показывает эти цифры как «устаревшие» (серым), пока не придёт свежий ответ claude.ai.
//
// Честность: если время сброса окна уже прошло, его процент обнуляем (после сброса так и
// есть, пока не начнёшь новое окно). Кеш старше 8 дней не используем — неделя точно сменилась.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::usage::{Limit, Usage};

const FILE: &str = "last-usage.json";
const MAX_AGE_MS: u64 = 8 * 24 * 3600 * 1000;

static PATH: OnceLock<PathBuf> = OnceLock::new();
/// Последнее записанное содержимое — чтобы не писать файл, если ничего не поменялось
static LAST: Mutex<Option<String>> = Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    saved_at_ms: u64,
    usage: Usage,
}

/// Вызвать один раз при старте с папкой профиля.
pub fn init(cfg_dir: PathBuf) {
    let _ = PATH.set(cfg_dir.join(FILE));
}

/// Сохранить свежие цифры (молча; ошибка записи кеша ничего не ломает).
pub fn save(usage: &Usage, now_ms: u64) {
    let Some(path) = PATH.get() else { return };
    // Сравниваем без времени: одинаковые цифры каждые 30 с на диск не пишем
    let Ok(key) = serde_json::to_string(usage) else { return };
    {
        let mut last = LAST.lock().unwrap();
        if last.as_deref() == Some(key.as_str()) {
            return;
        }
        *last = Some(key);
    }
    if let Ok(text) = serde_json::to_string(&Stored { saved_at_ms: now_ms, usage: usage.clone() }) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

/// Прочитать кеш: (цифры, когда сохранены). None — кеша нет, он битый или слишком старый.
pub fn load(now_ms: u64) -> Option<(Usage, u64)> {
    let text = fs::read_to_string(PATH.get()?).ok()?;
    parse(&text, now_ms)
}

fn parse(text: &str, now_ms: u64) -> Option<(Usage, u64)> {
    let s: Stored = serde_json::from_str(text).ok()?;
    if s.saved_at_ms > now_ms || now_ms - s.saved_at_ms > MAX_AGE_MS {
        return None;
    }
    let mut u = s.usage;
    u.source = "cache";
    let fix = |l: &mut Limit| {
        if let Some(t) = l.resets_at.as_deref().and_then(iso_ms) {
            if t <= now_ms {
                l.percent = 0.0;
                l.resets_at = None;
                l.active = false;
                l.severity = None;
            }
        }
    };
    if let Some(l) = u.session.as_mut() { fix(l); }
    if let Some(l) = u.weekly.as_mut() { fix(l); }
    for l in u.others.iter_mut() { fix(l); }
    Some((u, s.saved_at_ms))
}

/// ISO 8601 → мс от 1970 (UTC). Понимает «2026-10-30T12:00:00Z», дробные секунды и
/// смещение «+03:00». Чего не понимает — None (тогда окно не трогаем).
pub fn iso_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let mut rest = s.get(19..)?;
    let mut ms = 0i64;
    if let Some(r) = rest.strip_prefix('.') {
        let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        let frac = format!("{:0<3}", &digits[..digits.len().min(3)]);
        ms = frac.parse().ok()?;
        rest = &r[digits.len()..];
    }
    let offset_min = match rest {
        "" | "Z" | "z" => 0,
        o if o.len() == 6 && (o.starts_with('+') || o.starts_with('-')) => {
            let v = o.get(1..3)?.parse::<i64>().ok()? * 60 + o.get(4..6)?.parse::<i64>().ok()?;
            if o.starts_with('-') { -v } else { v }
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    let secs = days * 86_400 + h * 3600 + mi * 60 + se - offset_min * 60;
    u64::try_from(secs * 1000 + ms).ok()
}

/// (год, месяц, день) → дни от 1970-01-01. Алгоритм Говарда Хиннанта.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lim(kind: &str, p: f64, at: Option<&str>) -> Limit {
        Limit { kind: kind.into(), percent: p, resets_at: at.map(Into::into), severity: None, active: true }
    }

    #[test]
    fn iso_variants() {
        assert_eq!(iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_ms("2026-10-30T12:00:00Z"), Some(1_793_361_600_000));
        assert_eq!(iso_ms("2026-10-30T12:00:00.5+00:00"), Some(1_793_361_600_500));
        assert_eq!(iso_ms("2026-10-30T15:00:00+03:00"), Some(1_793_361_600_000));
        assert_eq!(iso_ms("2026-10-30T12:00:00.123456Z"), Some(1_793_361_600_123));
        assert_eq!(iso_ms("вчера"), None);
    }

    #[test]
    fn round_trip_and_reset() {
        let now = iso_ms("2026-10-30T12:00:00Z").unwrap();
        let u = Usage {
            session: Some(lim("session", 42.0, Some("2026-10-30T11:00:00Z"))), // уже сброшено
            weekly: Some(lim("weekly_all", 18.0, Some("2026-11-02T08:00:00Z"))),
            others: vec![],
            breakdown: vec![],
            extra: None,
            source: "limits",
        };
        let text = serde_json::to_string(&Stored { saved_at_ms: now - 60_000, usage: u }).unwrap();
        let (c, at) = parse(&text, now).unwrap();
        assert_eq!(at, now - 60_000);
        assert_eq!(c.source, "cache");
        let s = c.session.unwrap();
        assert_eq!((s.percent, s.resets_at.is_none()), (0.0, true));
        assert_eq!(c.weekly.unwrap().percent, 18.0);
        // Слишком старый и «из будущего» — не берём
        assert!(parse(&text, now + MAX_AGE_MS).is_none());
        assert!(parse(&text, now - 120_000).is_none());
        assert!(parse("{битый", now).is_none());
    }
}
