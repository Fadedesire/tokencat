// Модель данных /usage и разбор ответа claude.ai (шаг S05).
//
// Источник неофициальный и меняется (CONTEXT.md), поэтому разбор терпимый:
//  - незнакомые ключи игнорируем;
//  - любое окно может быть null или отсутствовать;
//  - числа принимаем и целыми, и дробными, и строкой.
// Порядок: сначала массив `limits` (там свежие проценты и «серьёзность»), если в нём нет
// нужного окна — старые ключи `five_hour` / `seven_day`.
// Урок S04: `five_hour.utilization` может отставать (0% при 2% на сайте), поэтому `limits`
// всегда главнее.
//
// Без Tauri и сети — целиком покрыто тестами.

use serde::Serialize;
use serde_json::Value;

/// Один лимит: 5-часовое окно, неделя, неделя по модели и т.п.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limit {
    /// session | weekly_all | weekly_sonnet | weekly_opus | … (как пришло)
    pub kind: String,
    /// Израсходовано, 0–100
    pub percent: f64,
    /// Время сброса, ISO 8601 (как пришло; JS сам разбирает)
    pub resets_at: Option<String>,
    /// normal | warning | … (как пришло; может не быть)
    pub severity: Option<String>,
    /// Это окно сейчас ограничивает пользователя
    pub active: bool,
}

/// Доля недели по продуктам (Claude Code, Чаты, Cowork…).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Share {
    pub key: String,
    /// Уже на языке аккаунта
    pub name: String,
    pub percent: f64,
}

/// Кредиты сверх плана.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Extra {
    pub enabled: bool,
    pub percent: Option<f64>,
    /// Потрачено в минимальных единицах валюты (центы)
    pub used_minor: Option<i64>,
    pub currency: Option<String>,
    pub exponent: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Usage {
    /// 5-часовое окно
    pub session: Option<Limit>,
    /// Неделя, все модели
    pub weekly: Option<Limit>,
    /// Остальные окна (неделя по отдельным моделям и что появится потом)
    pub others: Vec<Limit>,
    pub breakdown: Vec<Share>,
    pub extra: Option<Extra>,
    /// limits | legacy — откуда взяты проценты (для лога)
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    NotObject,
    NoLimits,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NotObject => f.write_str("ответ /usage — не объект"),
            ParseError::NoLimits => f.write_str("в ответе /usage нет ни одного лимита"),
        }
    }
}

/// Число из JSON: 67, 67.5, "67" → 67.0. Всё прочее (null, NaN, текст) → None.
fn num(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().trim_end_matches('%').parse::<f64>().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn pct(n: f64) -> f64 {
    n.clamp(0.0, 100.0)
}

/// Элемент массива `limits`. Без `kind` или процента — пропускаем.
fn limit_from_item(v: &Value) -> Option<Limit> {
    Some(Limit {
        kind: text(&v["kind"])?,
        percent: pct(num(&v["percent"])?),
        resets_at: text(&v["resets_at"]),
        severity: text(&v["severity"]),
        active: v["is_active"].as_bool().unwrap_or(false),
    })
}

/// Старый формат: `{"utilization": 35, "resets_at": "…"}`.
fn limit_from_legacy(v: &Value, kind: &str) -> Option<Limit> {
    Some(Limit {
        kind: kind.into(),
        percent: pct(num(&v["utilization"])?),
        resets_at: text(&v["resets_at"]),
        severity: None,
        active: false,
    })
}

/// Старые ключи → вид окна в новой схеме.
const LEGACY: [(&str, &str); 4] = [
    ("five_hour", "session"),
    ("seven_day", "weekly_all"),
    ("seven_day_sonnet", "weekly_sonnet"),
    ("seven_day_opus", "weekly_opus"),
];

pub fn parse(v: &Value) -> Result<Usage, ParseError> {
    if !v.is_object() {
        return Err(ParseError::NotObject);
    }

    // 1. Новый массив limits
    let mut all: Vec<Limit> = v["limits"]
        .as_array()
        .map(|a| a.iter().filter_map(limit_from_item).collect())
        .unwrap_or_default();
    let from_limits = !all.is_empty();

    // 2. Чего нет в limits — добираем из старых ключей
    for (key, kind) in LEGACY {
        if all.iter().any(|l| l.kind == kind) {
            continue;
        }
        if let Some(l) = limit_from_legacy(&v[key], kind) {
            all.push(l);
        }
    }
    if all.is_empty() {
        return Err(ParseError::NoLimits);
    }

    let take = |all: &mut Vec<Limit>, kind: &str| {
        all.iter().position(|l| l.kind == kind).map(|i| all.remove(i))
    };
    let session = take(&mut all, "session");
    let weekly = take(&mut all, "weekly_all");
    // Повторы одного и того же вида не показываем дважды
    let mut others: Vec<Limit> = vec![];
    for l in all {
        if !others.iter().any(|o| o.kind == l.kind) {
            others.push(l);
        }
    }

    let breakdown = v["seven_day_breakdown"]["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let key = text(&r["key"])?;
                    Some(Share {
                        name: text(&r["display_name"]).unwrap_or_else(|| key.clone()),
                        key,
                        percent: pct(num(&r["percent"])?),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let extra = parse_extra(v);

    Ok(Usage {
        session,
        weekly,
        others,
        breakdown,
        extra,
        source: if from_limits { "limits" } else { "legacy" },
    })
}

/// Кредиты сверх плана: сначала новый `spend`, иначе старый `extra_usage`.
fn parse_extra(v: &Value) -> Option<Extra> {
    let s = &v["spend"];
    if s.is_object() {
        return Some(Extra {
            enabled: s["enabled"].as_bool().unwrap_or(false),
            percent: num(&s["percent"]).map(pct),
            used_minor: s["used"]["amount_minor"].as_i64(),
            currency: text(&s["used"]["currency"]),
            exponent: s["used"]["exponent"].as_u64().map(|e| e.min(6) as u32),
        });
    }
    let e = &v["extra_usage"];
    if e.is_object() {
        return Some(Extra {
            enabled: e["is_enabled"].as_bool().unwrap_or(false),
            percent: num(&e["utilization"]).map(pct),
            used_minor: None,
            currency: text(&e["currency"]),
            exponent: None,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Ответ, который реально пришёл в S02 (сокращён, ключи-кодовые имена оставлены).
    fn real_2026_10_29() -> Value {
        json!({
            "amber_cistern": null, "brass_thimble": null, "tangelo": null,
            "extra_usage": {"is_enabled": false, "monthly_limit": null, "utilization": null},
            "five_hour": {"limit_dollars": null, "locked_reason": null,
                          "resets_at": "2026-10-29T13:50:00.638330+00:00", "utilization": 0},
            "limits": [
                {"group": "session", "is_active": false, "kind": "session", "percent": 2,
                 "resets_at": "2026-10-29T13:50:00.638330+00:00", "scope": null, "severity": "normal"},
                {"group": "weekly", "is_active": true, "kind": "weekly_all", "percent": 83,
                 "resets_at": "2026-11-03T02:00:00.638349+00:00", "scope": null, "severity": "warning"}
            ],
            "seven_day": {"resets_at": "2026-11-03T02:00:00.638349+00:00", "utilization": 83},
            "seven_day_breakdown": {"rows": [
                {"display_name": "Claude Code", "key": "claude_code", "percent": 0},
                {"display_name": "Чаты", "key": "chat", "percent": 6},
                {"display_name": "Cowork", "key": "cowork", "percent": 94}
            ]},
            "seven_day_opus": null, "seven_day_sonnet": null,
            "spend": {"enabled": false, "percent": 0, "severity": "normal",
                      "used": {"amount_minor": 0, "currency": "USD", "exponent": 2}}
        })
    }

    #[test]
    fn real_response_prefers_limits_array() {
        let u = parse(&real_2026_10_29()).unwrap();
        assert_eq!(u.source, "limits");
        let s = u.session.unwrap();
        // В limits 2%, в five_hour 0% — верим limits (урок S04)
        assert_eq!(s.percent, 2.0);
        assert_eq!(s.severity.as_deref(), Some("normal"));
        let w = u.weekly.unwrap();
        assert_eq!(w.percent, 83.0);
        assert!(w.active);
        assert_eq!(w.resets_at.as_deref(), Some("2026-11-03T02:00:00.638349+00:00"));
        assert!(u.others.is_empty());
        assert_eq!(u.breakdown.len(), 3);
        assert_eq!(u.breakdown[1].name, "Чаты");
        let e = u.extra.unwrap();
        assert!(!e.enabled);
        assert_eq!(e.currency.as_deref(), Some("USD"));
    }

    #[test]
    fn legacy_format_from_context_md() {
        let u = parse(&json!({
            "five_hour": {"utilization": 35.0, "resets_at": "2026-10-29T15:00:00+00:00"},
            "seven_day": {"utilization": 74.0, "resets_at": "2026-11-04T10:00:00+00:00"},
            "seven_day_sonnet": {"utilization": 39.0, "resets_at": "x"},
            "seven_day_opus": null,
            "extra_usage": {"is_enabled": false, "monthly_limit": 0, "used_credits": 0.0}
        }))
        .unwrap();
        assert_eq!(u.source, "legacy");
        assert_eq!(u.session.unwrap().percent, 35.0);
        assert_eq!(u.weekly.unwrap().percent, 74.0);
        assert_eq!(u.others.len(), 1);
        assert_eq!(u.others[0].kind, "weekly_sonnet");
        assert!(u.breakdown.is_empty());
        assert!(!u.extra.unwrap().enabled);
    }

    #[test]
    fn null_windows_are_skipped() {
        let u = parse(&json!({
            "five_hour": null,
            "seven_day": {"utilization": 10, "resets_at": null},
            "limits": null
        }))
        .unwrap();
        assert!(u.session.is_none());
        let w = u.weekly.unwrap();
        assert_eq!(w.percent, 10.0);
        assert!(w.resets_at.is_none());
    }

    #[test]
    fn limits_fill_gaps_from_legacy() {
        // В limits только неделя — 5 часов берём из старого ключа
        let u = parse(&json!({
            "limits": [{"kind": "weekly_all", "percent": 50}],
            "five_hour": {"utilization": 12}
        }))
        .unwrap();
        assert_eq!(u.session.unwrap().percent, 12.0);
        assert_eq!(u.weekly.unwrap().percent, 50.0);
    }

    #[test]
    fn unknown_keys_and_kinds_are_tolerated() {
        let u = parse(&json!({
            "limits": [
                {"kind": "session", "percent": "7", "brand_new_field": {"a": 1}},
                {"kind": "weekly_scoped_magic", "percent": 40.5, "scope": {"model": "x"}},
                {"kind": "weekly_scoped_magic", "percent": 41},
                {"kind": "broken_no_percent"},
                {"percent": 99},
                "мусор",
                null
            ],
            "iguana_necktie": [1, 2, 3],
            "seven_day_breakdown": {"rows": [{"key": "chat"}, {"key": "x", "percent": 5}]},
            "spend": {"used": null}
        }))
        .unwrap();
        assert_eq!(u.session.unwrap().percent, 7.0);
        assert!(u.weekly.is_none());
        assert_eq!(u.others.len(), 1, "повтор вида не дублируется");
        assert_eq!(u.others[0].percent, 40.5);
        // Строка без процента пропущена, без имени — имя = ключ
        assert_eq!(u.breakdown.len(), 1);
        assert_eq!(u.breakdown[0].name, "x");
        let e = u.extra.unwrap();
        assert!(e.used_minor.is_none() && e.currency.is_none());
    }

    #[test]
    fn percent_is_clamped() {
        let u = parse(&json!({"limits": [
            {"kind": "session", "percent": 140},
            {"kind": "weekly_all", "percent": -3}
        ]}))
        .unwrap();
        assert_eq!(u.session.unwrap().percent, 100.0);
        assert_eq!(u.weekly.unwrap().percent, 0.0);
    }

    #[test]
    fn empty_or_wrong_shape_is_an_error() {
        assert_eq!(parse(&json!({})), Err(ParseError::NoLimits));
        assert_eq!(
            parse(&json!({"five_hour": null, "seven_day": null, "limits": []})),
            Err(ParseError::NoLimits)
        );
        assert_eq!(parse(&json!([])), Err(ParseError::NotObject));
        assert_eq!(parse(&json!("error")), Err(ParseError::NotObject));
        assert_eq!(parse(&Value::Null), Err(ParseError::NotObject));
    }
}
