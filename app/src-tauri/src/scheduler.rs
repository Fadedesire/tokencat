// Планировщик обновлений (S05).
//
// - Обновляет сразу при старте, потом раз в интервал из настроек (по умолчанию 30 с).
// - «Обновить» в UI, вход и смена прокси — обновление сразу (kick).
// - Выход компьютера из сна — обновление сразу: каждые 5 с сверяем настенные часы, и если
//   между «тиками» прошло намного больше 5 с, значит, компьютер спал.
// - Ошибки — откат: 429/5xx/VPN/формат — интервал ×2, ×4… до 5 минут; нет сети — пауза
//   15 с, 30 с, 1 мин… до 5 минут (не дёргаем сеть, пока её нет); сессия истекла или не
//   вошли — ждём входа, запросов нет.
// - Последние хорошие данные не выбрасываем при временной ошибке: UI показывает их как
//   «устаревшие» вместе с текстом ошибки.
//
// Результат каждого цикла — событие `usage-updated` окнам приложения (Snapshot, без секретов),
// подсказка значка в трее и, если пора, уведомление (alerts.rs).
// Раз в сутки после удачного цикла — имя аккаунта для шапки (B1, п. 9).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::api::{name_from_org, Failure, Net, OrgInfo};
use crate::{alerts, applog, shell, tray};
use crate::auth::key::SessionKey;
use crate::auth::verdict::Verdict;
use crate::auth::{emit_ui, state_for, Auth};
use crate::settings::{proxy_for_log, Settings, SettingsStore};
use crate::usage::Usage;

/// Потолок отката (SPEC: до 5 минут)
pub const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// Первая пауза без сети
const OFFLINE_FIRST: Duration = Duration::from_secs(15);
/// Шаг проверки «не спал ли компьютер»
const TICK: Duration = Duration::from_secs(5);
/// Если между тиками прошло больше TICK + это — считаем, что был сон
const SLEEP_GAP: Duration = Duration::from_secs(20);

/// Что делать после цикла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    After(Duration),
    /// Ждать входа / «Обновить» / пробуждения
    WaitForKick,
}

/// Чистая логика отката. `failures` — сколько ошибок подряд, включая эту (≥1 для ошибок).
pub fn plan_next(interval: Duration, outcome: Option<Verdict>, failures: u32) -> Plan {
    let grow = |base: Duration, n: u32| {
        let factor = 1u32 << n.min(10);
        base.saturating_mul(factor).min(MAX_BACKOFF)
    };
    match outcome {
        None | Some(Verdict::Ok) => Plan::After(interval),
        Some(Verdict::SessionExpired) => Plan::WaitForKick,
        Some(Verdict::Offline) | Some(Verdict::Proxy) => {
            Plan::After(grow(OFFLINE_FIRST, failures.saturating_sub(1)))
        }
        Some(Verdict::RateLimited)
        | Some(Verdict::ServerError)
        | Some(Verdict::Blocked)
        | Some(Verdict::Unexpected) => Plan::After(grow(interval, failures.max(1)).max(interval)),
    }
}

/// Был ли сон: настенное время ушло вперёд намного дальше, чем мы ждали.
pub fn woke_from_sleep(waited: Duration, wall_elapsed: Duration) -> bool {
    wall_elapsed > waited + SLEEP_GAP
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// То, что видит UI.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// loading | ok | signed_out | expired | blocked | offline | error
    pub state: &'static str,
    pub message: String,
    /// Последние хорошие данные (при ошибке — устаревшие)
    pub usage: Option<Usage>,
    pub org_name: Option<String>,
    /// Имя аккаунта claude.ai для шапки (без почты); None — показываем «TokenCat»
    pub account_name: Option<String>,
    /// Когда получены данные `usage` (мс от 1970)
    pub updated_at_ms: Option<u64>,
    /// Когда будет следующая попытка (мс от 1970); None — ждём входа
    pub next_at_ms: Option<u64>,
    pub fetching: bool,
    /// Показать ссылку «Открыть лог» (неожиданный ответ)
    pub show_log: bool,
}

impl Snapshot {
    fn loading() -> Self {
        Snapshot {
            state: "loading",
            message: "Загружаю данные…".into(),
            usage: None,
            org_name: None,
            account_name: None,
            updated_at_ms: None,
            next_at_ms: None,
            fetching: true,
            show_log: false,
        }
    }
}

pub struct Scheduler {
    kick: Notify,
    snap: Mutex<Snapshot>,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler { kick: Notify::new(), snap: Mutex::new(Snapshot::loading()) }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().unwrap().clone()
    }

    fn edit(&self, app: &AppHandle, f: impl FnOnce(&mut Snapshot)) {
        let name = app.state::<SettingsStore>().get().account_name;
        let s = {
            let mut g = self.snap.lock().unwrap();
            f(&mut g);
            g.account_name = if g.state == "signed_out" { None } else { name };
            g.clone()
        };
        tray::update(app, &s);
        emit_ui(app, "usage-updated", s);
    }

    /// Разослать текущий снимок ещё раз (например, когда пришло имя аккаунта).
    pub fn resend(&self, app: &AppHandle) {
        self.edit(app, |_| {});
    }
}

/// Попросить обновить сейчас (кнопка, вход, выход, смена прокси).
pub fn kick(app: &AppHandle) {
    if let Some(s) = app.try_state::<Scheduler>() {
        s.kick.notify_one();
    }
}

/// Главный цикл. Запускается один раз при старте программы.
pub async fn run(app: AppHandle) {
    let mut failures: u32 = 0;
    let mut plan = Plan::After(Duration::ZERO);
    loop {
        wait(&app, plan).await;
        let (p, f) = cycle(&app, failures).await;
        plan = p;
        failures = f;
    }
}

/// Ждём до срока, «пинка» или пробуждения из сна.
async fn wait(app: &AppHandle, plan: Plan) {
    let sched = app.state::<Scheduler>();
    let deadline = match plan {
        Plan::After(d) => Some(Instant::now() + d),
        Plan::WaitForKick => None,
    };
    loop {
        let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if left == Some(Duration::ZERO) {
            return;
        }
        let step = left.map_or(TICK, |l| l.min(TICK));
        let wall = SystemTime::now();
        tokio::select! {
            _ = sched.kick.notified() => return,
            _ = tokio::time::sleep(step) => {}
        }
        let wall_elapsed = SystemTime::now().duration_since(wall).unwrap_or_default();
        if woke_from_sleep(step, wall_elapsed) {
            applog::info(format!("компьютер проснулся (пауза {} с) — обновляю", wall_elapsed.as_secs()));
            return;
        }
    }
}

/// Один цикл: ключ → организация → /usage → снимок. Возвращает план и счётчик ошибок.
async fn cycle(app: &AppHandle, failures: u32) -> (Plan, u32) {
    let sched = app.state::<Scheduler>();
    let settings = app.state::<SettingsStore>().get();
    let interval = Duration::from_secs(settings.interval_secs);
    let auth = app.state::<Auth>();

    let key = match auth.current_key() {
        Ok(Some(k)) => k,
        Ok(None) => {
            sched.edit(app, |s| {
                *s = Snapshot {
                    state: "signed_out",
                    message: "Вы не вошли в claude.ai".into(),
                    fetching: false,
                    ..Snapshot::loading()
                }
            });
            return (Plan::WaitForKick, 0);
        }
        Err(e) => {
            applog::warn(format!("хранилище секретов: {e}"));
            let n = failures + 1;
            let plan = plan_next(interval, Some(Verdict::Unexpected), n);
            sched.edit(app, |s| {
                s.state = "error";
                s.message = e;
                s.fetching = false;
                s.show_log = true;
                s.next_at_ms = next_ms(plan);
            });
            return (plan, n);
        }
    };

    sched.edit(app, |s| s.fetching = true);
    let started = Instant::now();
    let result = fetch(app, &key, &settings).await;

    match result {
        Ok((usage, org)) => {
            if failures > 0 {
                applog::info("связь восстановлена");
            }
            let plan = plan_next(interval, None, 0);
            sched.edit(app, |s| {
                *s = Snapshot {
                    state: "ok",
                    message: Verdict::Ok.user_message().into(),
                    usage: Some(usage.clone()),
                    org_name: org.name.clone(),
                    account_name: None, // подставит edit
                    updated_at_ms: Some(now_ms()),
                    next_at_ms: next_ms(plan),
                    fetching: false,
                    show_log: false,
                };
            });
            alerts::on_usage(app, &usage);
            refresh_account_name(app, &key, &settings, &org).await;
            (plan, 0)
        }
        Err(f) => {
            let n = failures + 1;
            let plan = plan_next(interval, Some(f.verdict), n);
            // Одинаковую ошибку подряд в лог не льём: пишем первую и каждую десятую
            if n == 1 || n % 10 == 0 {
                applog::warn(format!(
                    "{:?}: {} (подряд {n}, {} мс, прокси: {}) → {}",
                    f.verdict,
                    f.detail,
                    started.elapsed().as_millis(),
                    proxy_for_log(settings.proxy.as_deref()),
                    describe(plan)
                ));
            }
            let expired = f.verdict == Verdict::SessionExpired;
            sched.edit(app, |s| {
                s.state = state_for(f.verdict);
                s.message = f.verdict.user_message().into();
                s.fetching = false;
                s.show_log = f.verdict == Verdict::Unexpected;
                s.next_at_ms = next_ms(plan);
                if expired {
                    // Чужие (старые) цифры после истёкшей сессии не показываем
                    s.usage = None;
                    s.updated_at_ms = None;
                }
            });
            (plan, n)
        }
    }
}

const DAY_SECS: u64 = 24 * 3600;
const HOUR_SECS: u64 = 3600;
/// Когда последний раз пробовали получить имя (секунды): при ошибке — не чаще раза в час
static NAME_TRY: AtomicU64 = AtomicU64::new(0);

/// Имя аккаунта: при входе и раз в сутки, не каждый цикл (бережём claude.ai).
async fn refresh_account_name(app: &AppHandle, key: &SessionKey, settings: &Settings, org: &OrgInfo) {
    let now = now_ms() / 1000;
    if settings.account_name_at.is_some_and(|at| now.saturating_sub(at) < DAY_SECS) {
        return;
    }
    let last = NAME_TRY.load(Ordering::SeqCst);
    if last != 0 && now.saturating_sub(last) < HOUR_SECS {
        return;
    }
    NAME_TRY.store(now, Ordering::SeqCst);
    let fallback = || org.name.as_deref().and_then(name_from_org);
    let got = match Net::new(settings.proxy.as_deref()) {
        Ok(net) => net.account_name(key).await,
        Err(f) => Err(f),
    };
    let r = match got {
        Ok(name) => {
            let name = name.or_else(fallback);
            applog::info(if name.is_some() { "имя аккаунта обновлено" } else { "имени аккаунта нет" });
            app.state::<SettingsStore>().update(|s| {
                s.account_name = name;
                s.account_name_at = Some(now);
            })
        }
        Err(f) => {
            // /api/account неофициальный: если он сломается, берём название организации
            applog::warn(format!("имя аккаунта не получено: {}", f.detail));
            if settings.account_name.is_some() {
                return;
            }
            app.state::<SettingsStore>().update(|s| s.account_name = fallback())
        }
    };
    if let Err(e) = r {
        applog::warn(e);
    }
    app.state::<Scheduler>().resend(app);
    shell::settings_changed(app);
}

fn next_ms(plan: Plan) -> Option<u64> {
    match plan {
        Plan::After(d) => Some(now_ms() + d.as_millis() as u64),
        Plan::WaitForKick => None,
    }
}

fn describe(plan: Plan) -> String {
    match plan {
        Plan::After(d) => format!("повтор через {} с", d.as_secs()),
        Plan::WaitForKick => "жду входа".into(),
    }
}

/// Организация (из памяти, прошлого запуска или списка) + /usage.
/// Если /usage для сохранённой организации не отвечает (организацию сменили) —
/// один раз заново выбираем организацию.
async fn fetch(app: &AppHandle, key: &SessionKey, settings: &Settings) -> Result<(Usage, OrgInfo), Failure> {
    let auth = app.state::<Auth>();
    let net = Net::new(settings.proxy.as_deref())?;

    let org = match auth.org() {
        Some(o) => o,
        None => {
            let o = net.verify(key, settings.last_org.as_deref()).await?;
            remember_org(app, &o);
            o
        }
    };
    match net.usage(key, &org.uuid).await {
        Ok(u) => {
            if settings.last_org.as_deref() != Some(org.uuid.as_str()) {
                remember_org(app, &org);
            }
            Ok((u, org))
        }
        Err(f) if f.verdict == Verdict::SessionExpired || f.status == Some(404) => {
            // Ключ плохой — verify тоже вернёт SessionExpired, и мы честно попросим войти
            let o2 = net.verify(key, None).await?;
            if o2.uuid == org.uuid {
                return Err(f);
            }
            applog::info("организация сменилась — беру новую");
            remember_org(app, &o2);
            let u = net.usage(key, &o2.uuid).await?;
            Ok((u, o2))
        }
        Err(f) => Err(f),
    }
}

fn remember_org(app: &AppHandle, o: &OrgInfo) {
    app.state::<Auth>().set_org(o.clone());
    let uuid = o.uuid.clone();
    if let Err(e) = app.state::<SettingsStore>().update(|s| s.last_org = Some(uuid)) {
        applog::warn(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn ok_uses_interval() {
        assert_eq!(plan_next(S(30), None, 0), Plan::After(S(30)));
        assert_eq!(plan_next(S(60), None, 0), Plan::After(S(60)));
        assert_eq!(plan_next(S(30), Some(Verdict::Ok), 0), Plan::After(S(30)));
    }

    #[test]
    fn rate_limit_backs_off_up_to_5_min() {
        let p = |n| plan_next(S(60), Some(Verdict::RateLimited), n);
        assert_eq!(p(1), Plan::After(S(120)));
        assert_eq!(p(2), Plan::After(S(240)));
        assert_eq!(p(3), Plan::After(S(300)));
        assert_eq!(p(50), Plan::After(S(300)));
        // Интервал 5 мин: откат не делает реже, чем сам интервал
        assert_eq!(plan_next(S(300), Some(Verdict::ServerError), 1), Plan::After(S(300)));
        assert_eq!(plan_next(S(30), Some(Verdict::Blocked), 1), Plan::After(S(60)));
        assert_eq!(plan_next(S(30), Some(Verdict::Unexpected), 4), Plan::After(S(300)));
    }

    #[test]
    fn offline_pauses_gently() {
        let p = |n| plan_next(S(60), Some(Verdict::Offline), n);
        assert_eq!(p(1), Plan::After(S(15)));
        assert_eq!(p(2), Plan::After(S(30)));
        assert_eq!(p(3), Plan::After(S(60)));
        assert_eq!(p(6), Plan::After(S(300)));
        assert_eq!(plan_next(S(60), Some(Verdict::Proxy), 1), Plan::After(S(15)));
    }

    #[test]
    fn expired_session_stops_polling() {
        assert_eq!(plan_next(S(60), Some(Verdict::SessionExpired), 1), Plan::WaitForKick);
    }

    #[test]
    fn sleep_detection() {
        assert!(!woke_from_sleep(S(5), S(5)));
        assert!(!woke_from_sleep(S(5), S(12))); // компьютер просто тормозил
        assert!(woke_from_sleep(S(5), S(3600)));
    }
}
