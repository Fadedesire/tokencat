// Виды программы (B2) и всё, что их переключает.
//
// - overlay  — «Поверх окон»: главное окно = плавающая панель (overlay.rs).
// - taskbar  — Windows, «В панели задач»: виджет внутри панели задач (taskbar.rs); если
//              встроиться не удалось — проценты рисуются на значке в трее (icons.rs).
// - menubar  — macOS, «В строке меню»: котик + «5ч 42% · Н 18%» рядом с часами.
// В видах taskbar/menubar главное окно — всплывающая панель: появляется по клику у виджета
// (или значка), прячется, когда фокус ушёл в другое окно. Компонент панели один и тот же.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

use crate::auth::MAIN_LABEL;
use crate::icons::{self, Mood};
use crate::overlay::{self, Rect};
use crate::scheduler::{Scheduler, Snapshot};
use crate::settings::SettingsStore;
use crate::{applog, shell, tray};

/// К чему прикреплена всплывающая панель (виджет или значок), физические пиксели.
static ANCHOR: Mutex<Option<Rect>> = Mutex::new(None);
/// Когда панель спрятали из-за потери фокуса: клик по виджету в этот же момент — это
/// «закрыть», а не «открыть снова».
static LAST_HIDE: Mutex<Option<Instant>> = Mutex::new(None);
/// Что сейчас нарисовано на значке (чтобы не перерисовывать одно и то же)
static ICON_KEY: Mutex<String> = Mutex::new(String::new());
/// Windows: виджет в панели задач не встроился — показываем проценты на значке
static FALLBACK: AtomicBool = AtomicBool::new(false);
/// Зазор между панелью и виджетом, логические пиксели
const POPUP_GAP: f64 = 8.0;

pub fn current(app: &AppHandle) -> String {
    app.state::<SettingsStore>().get().view
}

/// Главное окно сейчас — всплывающая панель (а не плавающая)?
pub fn is_popup(app: &AppHandle) -> bool {
    current(app) != "overlay"
}

// ---------- Данные для значков ----------

/// Коротко о снимке: что рисовать на виджете и значке.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub session: Option<f64>,
    pub weekly: Option<f64>,
    pub stale: bool,
    pub mood: Mood,
}

pub fn summary(snap: &Snapshot) -> Summary {
    let none = Summary { session: None, weekly: None, stale: false, mood: Mood::Sleep };
    if snap.state == "signed_out" {
        return none;
    }
    let Some(u) = &snap.usage else { return none };
    let session = u.session.as_ref().map(|l| l.percent);
    let weekly = u.weekly.as_ref().map(|l| l.percent);
    let all: Vec<f64> = session.iter().chain(weekly.iter()).copied().chain(u.others.iter().map(|l| l.percent)).collect();
    if all.is_empty() {
        return none;
    }
    let max = all.iter().copied().fold(0.0, f64::max);
    Summary {
        session,
        weekly,
        stale: snap.state != "ok",
        mood: if max >= 80.0 { Mood::Alert } else { Mood::Idle },
    }
}

/// Текст в строке меню macOS. Пока по-русски, как и весь интерфейс (языки — B3).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn menubar_title(style: &str, s: &Summary) -> Option<String> {
    let pct = |v: Option<f64>| match v {
        Some(p) => format!("{}%", p.round().clamp(0.0, 999.0) as i64),
        None => "—".into(),
    };
    match style {
        "session" => Some(format!("5ч {}", pct(s.session))),
        "both" => Some(match (s.session, s.weekly) {
            (_, None) => format!("5ч {}", pct(s.session)),
            _ => format!("5ч {} · Н {}", pct(s.session), pct(s.weekly)),
        }),
        _ => None, // icon, ring — только значок
    }
}

// ---------- Где показать всплывающую панель ----------

/// Левый верхний угол панели размером w × h рядом с якорем на мониторе `mon`.
/// Панель встаёт с той стороны якоря, что дальше от края экрана: виджет внизу — над ним,
/// значок строки меню вверху — под ним, вертикальная панель задач слева — справа от неё.
pub fn popup_spot(anchor: Rect, w: i32, h: i32, mon: Rect, gap: i32) -> (i32, i32) {
    let cx = anchor.x + anchor.w / 2;
    let cy = anchor.y + anchor.h / 2;
    let clamp_x = |x: i32| x.min(mon.x + mon.w - w - gap).max(mon.x + gap);
    let clamp_y = |y: i32| y.min(mon.y + mon.h - h - gap).max(mon.y + gap);
    let to_bottom = mon.y + mon.h - (anchor.y + anchor.h);
    let to_top = anchor.y - mon.y;
    let to_left = anchor.x - mon.x;
    let to_right = mon.x + mon.w - (anchor.x + anchor.w);
    let nearest = to_bottom.min(to_top).min(to_left).min(to_right);
    if nearest == to_bottom {
        (clamp_x(cx - w / 2), clamp_y(anchor.y - h - gap))
    } else if nearest == to_top {
        (clamp_x(cx - w / 2), clamp_y(anchor.y + anchor.h + gap))
    } else if nearest == to_left {
        (clamp_x(anchor.x + anchor.w + gap), clamp_y(cy - h / 2))
    } else {
        (clamp_x(anchor.x - w - gap), clamp_y(cy - h / 2))
    }
}

fn monitor_for(app: &AppHandle, r: Rect) -> Option<(Rect, f64)> {
    let w = app.get_webview_window(MAIN_LABEL)?;
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    let mons = w.available_monitors().unwrap_or_default();
    let found = mons.iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        cx >= p.x && cy >= p.y && cx < p.x + s.width as i32 && cy < p.y + s.height as i32
    });
    let m = match found {
        Some(m) => m.clone(),
        None => w.primary_monitor().ok().flatten()?,
    };
    let (p, s) = (m.position(), m.size());
    Some((Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }, m.scale_factor()))
}

/// Прямоугольник tauri (физический или логический) → физические пиксели.
fn to_rect(r: &tauri::Rect, scale: f64) -> Rect {
    let p: PhysicalPosition<i32> = r.position.to_physical(scale);
    let s: PhysicalSize<u32> = r.size.to_physical(scale);
    Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }
}

fn main_scale(app: &AppHandle) -> f64 {
    app.get_webview_window(MAIN_LABEL).and_then(|w| w.scale_factor().ok()).unwrap_or(1.0)
}

/// Куда прикрепить панель, если клика не было (повторный запуск, пункт меню).
fn default_anchor(app: &AppHandle) -> Option<Rect> {
    #[cfg(windows)]
    if let Some(r) = crate::taskbar::widget_rect() {
        return Some(r);
    }
    if let Some(t) = app.tray_by_id(tray::TRAY_ID) {
        if let Ok(Some(r)) = t.rect() {
            let r = to_rect(&r, main_scale(app));
            if r.w > 0 && r.h > 0 {
                return Some(r);
            }
        }
    }
    // Ничего не знаем — угол основного монитора: Windows — снизу справа, macOS — сверху справа
    let w = app.get_webview_window(MAIN_LABEL)?;
    let m = w.primary_monitor().ok().flatten()?;
    let (p, s) = (m.position(), m.size());
    let y = if cfg!(target_os = "macos") { p.y } else { p.y + s.height as i32 - 1 };
    Some(Rect { x: p.x + s.width as i32 - 2, y, w: 1, h: 1 })
}

/// Поставить всплывающую панель у якоря (после показа и при смене её размера).
pub fn place_popup(app: &AppHandle) {
    let Some(anchor) = *ANCHOR.lock().unwrap() else { return };
    let Some(w) = app.get_webview_window(MAIN_LABEL) else { return };
    let Some((mon, scale)) = monitor_for(app, anchor) else { return };
    let size = w.outer_size().unwrap_or_default();
    let (x, y) = popup_spot(anchor, size.width as i32, size.height as i32, mon, (POPUP_GAP * scale) as i32);
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

/// Показать всплывающую панель. `anchor` — к чему прикрепить (None — к виджету/значку);
/// `toggle` — клик по виджету: если панель открыта, закрыть.
pub fn show_popup(app: &AppHandle, anchor: Option<Rect>, toggle: bool) {
    if !app.state::<SettingsStore>().get().onboarded {
        shell::show_panel(app);
        return;
    }
    let Some(w) = app.get_webview_window(MAIN_LABEL) else { return };
    if toggle {
        if w.is_visible().unwrap_or(false) {
            hide_popup(app);
            return;
        }
        // Панель только что спряталась из-за этого же клика (фокус ушёл на панель задач)
        let just_hidden = LAST_HIDE.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(400));
        if just_hidden {
            return;
        }
    }
    let anchor = anchor.or_else(|| default_anchor(app));
    *ANCHOR.lock().unwrap() = anchor;
    place_popup(app);
    let _ = w.set_always_on_top(true);
    let _ = w.show();
    let _ = w.set_focus();
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        let _ = w.hide();
    }
    *LAST_HIDE.lock().unwrap() = Some(Instant::now());
}

/// Фокус главного окна: всплывающая панель прячется, когда щёлкнули мимо.
pub fn on_focus(app: &AppHandle, focused: bool) {
    if focused || !is_popup(app) {
        return;
    }
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        if w.is_visible().unwrap_or(false) {
            hide_popup(app);
        }
    }
}

/// ЛКМ по значку в трее / строке меню.
pub fn on_tray_click(app: &AppHandle, rect: Option<tauri::Rect>) {
    if !is_popup(app) {
        shell::show_panel(app);
        return;
    }
    let scale = main_scale(app);
    let r = rect.map(|r| to_rect(&r, scale)).filter(|r| r.w > 0 && r.h > 0);
    // Windows: даже при клике по значку трея прикрепляем панель к виджету, если он есть
    #[cfg(windows)]
    let r = crate::taskbar::widget_rect().or(r);
    let app = app.clone();
    tauri::async_runtime::spawn(async move { show_popup(&app, r, true) });
}

// ---------- Переключение вида ----------

/// Вид сменился (настройки, меню, онбординг): переставить окна и значки.
pub fn apply(app: &AppHandle) {
    let s = app.state::<SettingsStore>().get();
    applog::info(format!("вид: {}", s.view));
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        if s.view != "overlay" {
            let _ = w.hide();
            let _ = w.set_always_on_top(true);
        } else {
            let _ = w.set_always_on_top(s.always_on_top);
            overlay::place(&w, app);
            if s.onboarded {
                let _ = w.show();
            }
        }
    }
    start_platform(app, &s.view);
    ICON_KEY.lock().unwrap().clear();
    refresh(app);
}

/// Старт программы: окна уже настроил overlay::setup, включаем вид ОС и значки.
pub fn init(app: &AppHandle) {
    let view = current(app);
    start_platform(app, &view);
    refresh(app);
}

#[cfg(windows)]
fn start_platform(app: &AppHandle, view: &str) {
    use crate::taskbar::{self, Event};
    let on = view == "taskbar";
    if on {
        let h = app.clone();
        taskbar::start(move |e| {
            // Поток виджета связан с Проводником: здесь ничего не ждём, всё — в задачу
            let h = h.clone();
            tauri::async_runtime::spawn(async move {
                match e {
                    Event::Click(r) => show_popup(&h, Some(r), true),
                    Event::Menu(x, y) => context_menu_at(&h, x, y),
                    Event::Embedded(ok) => {
                        FALLBACK.store(!ok, Ordering::SeqCst);
                        ICON_KEY.lock().unwrap().clear();
                        refresh(&h);
                    }
                }
            });
        });
    } else {
        FALLBACK.store(false, Ordering::SeqCst);
    }
    taskbar::set_enabled(on);
}

#[cfg(not(windows))]
fn start_platform(_app: &AppHandle, _view: &str) {}

/// Меню программы в точке экрана (ПКМ по виджету панели задач).
#[cfg(windows)]
fn context_menu_at(app: &AppHandle, x: i32, y: i32) {
    let Some(w) = app.get_webview_window(MAIN_LABEL) else { return };
    let menu = match tray::build_menu(app) {
        Ok(m) => m,
        Err(e) => {
            applog::warn(format!("меню: {e}"));
            return;
        }
    };
    // Позиция меню — относительно окна панели (даже скрытого)
    let p = w.inner_position().unwrap_or_default();
    if let Err(e) = w.popup_menu_at(&menu, PhysicalPosition::new(x - p.x, y - p.y)) {
        applog::warn(format!("меню у виджета: {e}"));
    }
}

// ---------- Значки и виджет ----------

/// Новый снимок от планировщика.
pub fn on_snapshot(app: &AppHandle, snap: &Snapshot) {
    tray::update(app, snap);
    refresh_with(app, snap);
}

/// Перерисовать виджет и значок по последнему снимку (сменились настройки).
pub fn refresh(app: &AppHandle) {
    let snap = app.state::<Scheduler>().snapshot();
    refresh_with(app, &snap);
}

fn set_icon(app: &AppHandle, key: String, make: impl FnOnce() -> Option<(Image<'static>, bool)>, title: Option<String>) {
    // Значка может ещё не быть (первый цикл планировщика идёт параллельно со стартом)
    let Some(t) = app.tray_by_id(tray::TRAY_ID) else { return };
    {
        let mut last = ICON_KEY.lock().unwrap();
        if *last == key {
            return;
        }
        *last = key;
    }
    if let Some((img, template)) = make() {
        let _ = t.set_icon(Some(img));
        #[cfg(target_os = "macos")]
        let _ = t.set_icon_as_template(template);
        #[cfg(not(target_os = "macos"))]
        let _ = template;
    }
    #[cfg(target_os = "macos")]
    let _ = t.set_title(title);
    #[cfg(not(target_os = "macos"))]
    let _ = title;
}

fn image(r: icons::Rgba) -> Image<'static> {
    Image::new_owned(r.px, r.w, r.h)
}

fn refresh_with(app: &AppHandle, snap: &Snapshot) {
    let s = app.state::<SettingsStore>().get();
    let sum = summary(snap);

    #[cfg(windows)]
    {
        if s.view == "taskbar" {
            crate::taskbar::update(crate::taskbar::Data {
                session: sum.session,
                weekly: sum.weekly,
                stale: sum.stale,
                mood: sum.mood,
                cat: s.show_cat,
            });
        }
        if s.view == "taskbar" && FALLBACK.load(Ordering::SeqCst) {
            // Запасной вид: проценты прямо на значке
            let key = format!("numbers {:?} {:?} {}", sum.session.map(|v| v.round()), sum.weekly.map(|v| v.round()), sum.stale);
            set_icon(app, key, || Some((image(icons::tray_numbers(sum.session, sum.weekly, sum.stale)), false)), None);
        } else {
            let a = app.clone();
            set_icon(app, "default".into(), move || a.default_window_icon().cloned().map(|i| (i.to_owned(), false)), None);
        }
    }

    #[cfg(target_os = "macos")]
    {
        const SIDE: u32 = 36; // 18 pt × 2 (Retina)
        let menubar = s.view == "menubar";
        let style = if menubar { s.menubar_style.as_str() } else { "icon" };
        let title = if menubar { menubar_title(style, &sum) } else { None };
        let key = format!("{style} {:?} {:?}", sum.session.map(|v| v.round()), title);
        let session = sum.session;
        let colored = menubar && session.is_some_and(|p| p >= 50.0);
        set_icon(
            app,
            key,
            move || {
                Some(if style == "ring" {
                    (image(icons::ring(SIDE, session)), false)
                } else if colored {
                    // 50 %+ — силуэт цветом порога (жёлтый/красный), иначе шаблонный
                    (image(icons::cat_silhouette(SIDE, session.map(icons::level_color))), false)
                } else {
                    (image(icons::cat_silhouette(SIDE, None)), true)
                })
            },
            title,
        );
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (s, sum);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Limit, Usage};

    const MON: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

    #[test]
    fn popup_above_taskbar_widget() {
        // Виджет в панели задач снизу, левее трея
        let widget = Rect { x: 1580, y: 1032, w: 120, h: 48 };
        let (x, y) = popup_spot(widget, 252, 140, MON, 8);
        assert_eq!(y, 1032 - 140 - 8);
        assert_eq!(x, 1580 + 60 - 126);
        // Значок трея у самого края — панель не вылезает за экран
        let icon = Rect { x: 1890, y: 1032, w: 24, h: 48 };
        assert_eq!(popup_spot(icon, 252, 140, MON, 8).0, 1920 - 252 - 8);
    }

    #[test]
    fn popup_below_menubar_and_beside_vertical_taskbar() {
        // macOS: значок в строке меню сверху
        let item = Rect { x: 1500, y: 0, w: 60, h: 48 };
        assert_eq!(popup_spot(item, 252, 140, MON, 8).1, 48 + 8);
        // Вертикальная панель задач слева
        let left = Rect { x: 0, y: 800, w: 62, h: 90 };
        let (x, y) = popup_spot(left, 252, 140, MON, 8);
        assert_eq!(x, 62 + 8);
        assert_eq!(y, 845 - 70);
        // …и справа
        let right = Rect { x: 1858, y: 800, w: 62, h: 90 };
        assert_eq!(popup_spot(right, 252, 140, MON, 8).0, 1858 - 252 - 8);
        // Второй монитор справа от основного
        let mon2 = Rect { x: 1920, y: 0, w: 2560, h: 1440 };
        let w2 = Rect { x: 4000, y: 1392, w: 120, h: 48 };
        let (x2, y2) = popup_spot(w2, 252, 140, mon2, 8);
        assert!(x2 >= 1920 && x2 + 252 <= 4480 && y2 == 1392 - 148);
    }

    fn lim(kind: &str, p: f64) -> Limit {
        Limit { kind: kind.into(), percent: p, resets_at: None, severity: None, active: false }
    }

    fn snap(state: &'static str, usage: Option<Usage>) -> Snapshot {
        Snapshot {
            state,
            message: String::new(),
            usage,
            org_name: None,
            account_name: None,
            updated_at_ms: None,
            next_at_ms: None,
            fetching: false,
            show_log: false,
        }
    }

    fn usage(s: f64, w: f64, other: Option<f64>) -> Usage {
        Usage {
            session: Some(lim("session", s)),
            weekly: Some(lim("weekly_all", w)),
            others: other.map(|p| vec![lim("weekly_opus", p)]).unwrap_or_default(),
            breakdown: vec![],
            extra: None,
            source: "limits",
        }
    }

    #[test]
    fn summary_and_title() {
        let ok = summary(&snap("ok", Some(usage(41.6, 18.0, None))));
        assert_eq!((ok.session, ok.weekly, ok.stale, ok.mood), (Some(41.6), Some(18.0), false, Mood::Idle));
        assert_eq!(menubar_title("both", &ok).as_deref(), Some("5ч 42% · Н 18%"));
        assert_eq!(menubar_title("session", &ok).as_deref(), Some("5ч 42%"));
        assert_eq!(menubar_title("icon", &ok), None);
        assert_eq!(menubar_title("ring", &ok), None);

        // Тревога — по любому окну, даже по «другим»
        assert_eq!(summary(&snap("ok", Some(usage(10.0, 20.0, Some(85.0))))).mood, Mood::Alert);
        // Нет сети — данные устарели, но остались
        let off = summary(&snap("offline", Some(usage(10.0, 20.0, None))));
        assert!(off.stale && off.session == Some(10.0));
        // Вышли — котик спит, процентов нет
        let out = summary(&snap("signed_out", Some(usage(10.0, 20.0, None))));
        assert_eq!((out.session, out.mood), (None, Mood::Sleep));
        assert_eq!(menubar_title("both", &out).as_deref(), Some("5ч —"));
    }
}
