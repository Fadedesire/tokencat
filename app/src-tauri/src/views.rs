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
    appear(app, &w);
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

// ---------- Показ и скрытие панели ----------

/// Панель отрисовалась после показа (main.js, команда panel_painted)
static PAINTED: AtomicBool = AtomicBool::new(false);

/// Вылет из панели задач (B10): следующий показ панели — «выплеском» из этой точки окна
/// (физические пиксели от левого верхнего угла).
static FLY_FROM: Mutex<Option<(i32, i32)>> = Mutex::new(None);

/// Залёт в панель задач: сколько длится втягивание панели (то же время — в overlay.css)
#[cfg(windows)]
const FLY_IN_MS: u64 = 190;

/// Показать главное окно. JS возвращает панели видимость (её гасит залёт) и через два
/// кадра сообщает «нарисовалась» — до этого системное перетаскивание не начинаем.
/// После вылета панель «выплёскивается» из точки FLY_FROM: пока окно WebView дорисовывается,
/// панель ещё прозрачная — анимация прикрывает этот лаг.
fn appear(app: &AppHandle, w: &tauri::WebviewWindow) {
    PAINTED.store(false, Ordering::SeqCst);
    let payload = match FLY_FROM.lock().unwrap().take() {
        Some((x, y)) => serde_json::json!({ "fly": [x, y] }),
        None => serde_json::Value::Null,
    };
    let _ = tauri::Emitter::emit_to(app, MAIN_LABEL, "panel-appear", payload);
    let _ = w.show();
}

/// Команда от main.js: панель показана и нарисована.
#[tauri::command]
pub fn panel_painted(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != MAIN_LABEL {
        return Err("forbidden".into());
    }
    PAINTED.store(true, Ordering::SeqCst);
    Ok(())
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
                appear(app, &w);
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
        let slot = app.state::<crate::profile::Profile>().slot();
        let order = app.state::<SettingsStore>().get().taskbar_order.unwrap_or(taskbar::default_order(slot));
        taskbar::set_order(order);
        taskbar::start(slot, order, move |e| {
            // Поток виджета связан с Проводником: здесь ничего не ждём, всё — в задачу
            if let Event::FollowTo(x, y) = e {
                follow_to(&h, x, y);
                return;
            }
            let h = h.clone();
            tauri::async_runtime::spawn(async move {
                match e {
                    Event::Click(r) => show_popup(&h, Some(r), true),
                    Event::Menu(x, y) => context_menu_at(&h, x, y),
                    Event::DragOut(x, y) => drag_out(&h, x, y),
                    Event::FollowTo(..) => {}
                    Event::Reorder(o) => {
                        if let Err(e) = h.state::<SettingsStore>().update(|s| s.taskbar_order = Some(o)) {
                            applog::warn(e);
                        }
                        applog::info(format!("панель задач: виджет переставлен, порядок {o}"));
                    }
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

/// Виджет вытянули из панели задач (поток виджета сообщил): плавающая панель под курсором,
/// дальше — watch_drag.
#[cfg(windows)]
fn drag_out(app: &AppHandle, _x: i32, _y: i32) {
    applog::info("виджет вытянули из панели задач");
    watch_drag(app, true);
}

/// Куда поставить ведомую панель (последняя точка) и есть ли уже переезд в очереди
#[cfg(windows)]
static FOLLOW_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
#[cfg(windows)]
static FOLLOW_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
#[cfg(windows)]
static FOLLOW_PENDING: AtomicBool = AtomicBool::new(false);

/// Поставить панель в точку (ничего не ждёт). В главный поток уходит не больше одного
/// переезда за раз, и он берёт самую свежую точку: если слать переезд на каждое движение
/// мыши, они копятся, и панель повторяет путь курсора с большим опозданием.
#[cfg(windows)]
fn follow_to(app: &AppHandle, x: i32, y: i32) {
    FOLLOW_X.store(x, Ordering::SeqCst);
    FOLLOW_Y.store(y, Ordering::SeqCst);
    if FOLLOW_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let h = app.clone();
    let r = app.run_on_main_thread(move || {
        FOLLOW_PENDING.store(false, Ordering::SeqCst);
        let (x, y) = (FOLLOW_X.load(Ordering::SeqCst), FOLLOW_Y.load(Ordering::SeqCst));
        if let Some(w) = h.get_webview_window(MAIN_LABEL) {
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
    });
    if r.is_err() {
        FOLLOW_PENDING.store(false, Ordering::SeqCst);
    }
}

#[cfg(windows)]
static WATCHING: AtomicBool = AtomicBool::new(false);

/// Зона «принести в панель задач» вокруг неё, логические пиксели. Вытащить обратно (или
/// «зарядить» вставку у панели, схваченной рядом с панелью задач) — увести курсор дальше,
/// чем на DOCK_ZONE + REARM_EXTRA: без дребезга на границе.
#[cfg(windows)]
const DOCK_ZONE: f64 = 36.0;
#[cfg(windows)]
const REARM_EXTRA: f64 = 24.0;

/// Курсор рядом с панелью задач (её прямоугольник, расширенный на `pad` во все стороны).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn near(tb: Rect, x: i32, y: i32, pad: i32) -> bool {
    x >= tb.x - pad && y >= tb.y - pad && x < tb.x + tb.w + pad && y < tb.y + tb.h + pad
}

/// Перетаскивание с зажатой ЛКМ. Решение Ромы 30.10: панель «влетела» в панель задач —
/// перетаскивание на этом заканчивается, как будто кнопку отпустили. Дальше мышь ни на что
/// не влияет, пока кнопку не отпустят; вытащить виджет снова — новым нажатием на него.
/// `from_widget` — зажали виджет и увели курсор из панели задач; иначе плавающую панель
/// тащит система. Отдельный поток раз в ~2 мс смотрит на курсор: вытаскивает, ведёт панель
/// за курсором (follow_to — без очереди переездов) и вставляет. Системное перетаскивание
/// при вставке прерываем (WM_CANCELMODE).
#[cfg(windows)]
pub fn watch_drag(app: &AppHandle, from_widget: bool) {
    use crate::taskbar;
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let r = std::thread::Builder::new().name("tokencat-drag".into()).spawn(move || {
        let scale = main_scale(&app);
        let zone = (DOCK_ZONE * scale) as i32;
        let rearm = zone + (REARM_EXTRA * scale) as i32;
        let (cx, cy) = taskbar::cursor_pos();
        // За какую точку держат панель: из виджета — середина шапки, иначе — где схватили
        let grab = match app.get_webview_window(MAIN_LABEL) {
            Some(w) if !from_widget => match w.outer_position() {
                Ok(p) => (cx - p.x, cy - p.y),
                Err(_) => (0, 0),
            },
            Some(w) => {
                let size = w.outer_size().unwrap_or_default();
                (size.width as i32 / 2, (13.0 * w.scale_factor().unwrap_or(1.0)) as i32)
            }
            None => (0, 0),
        };
        // Виджет сейчас в панели задач? Панель ведём сами? Вставка «заряжена»?
        let mut docked = from_widget;
        let mut lead = false;
        let mut armed = !taskbar::taskbar_rect().is_some_and(|tb| near(tb, cx, cy, zone));
        let mut last = (i32::MIN, i32::MIN);
        while taskbar::left_button_down() {
            let (x, y) = taskbar::cursor_pos();
            let tb = taskbar::taskbar_rect();
            if docked {
                // Вытаскивание из виджета — сразу (виджет уже решил, что курсор ушёл из
                // панели задач). Вставка в обратную сторону заканчивает цикл (break ниже),
                // так что «в панели» здесь бывает только в самом начале.
                if tb.is_some_and(|tb| !near(tb, x, y, 0)) {
                    undock(&app, x - grab.0, y - grab.1, grab);
                    docked = false;
                    lead = true;
                    // Только что вытащили: всасывание включится, когда курсор уйдёт из зоны
                    // с запасом (rearm) — иначе при медленном вытаскивании панель сразу
                    // влетала обратно (Рома, 30.10)
                    armed = false;
                    last = (x, y);
                }
            } else {
                if lead && (x, y) != last {
                    follow_to(&app, x - grab.0, y - grab.1);
                    last = (x, y);
                }
                match tb {
                    Some(tb) if !armed => armed = !near(tb, x, y, rearm),
                    Some(tb) if near(tb, x, y, zone) => {
                        // Влетела — сразу «отпускаем»: дальше этот поток мышь не смотрит
                        dock(&app, x, y);
                        break;
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        WATCHING.store(false, Ordering::SeqCst);
    });
    if r.is_err() {
        WATCHING.store(false, Ordering::SeqCst);
    }
}

/// Стать плавающей панелью: левый верхний угол в (x, y); `grab` — за какую точку панель
/// держат (из неё она «выплёскивается», B10).
#[cfg(windows)]
fn undock(app: &AppHandle, x: i32, y: i32, grab: (i32, i32)) {
    let r = app.state::<SettingsStore>().update(|s| {
        s.view = "overlay".into();
        s.overlay_x = Some(x);
        s.overlay_y = Some(y);
    });
    if let Err(e) = r {
        applog::warn(e);
        return;
    }
    applog::info("панель вытащили из панели задач");
    *FLY_FROM.lock().unwrap() = Some(grab);
    apply(app);
    shell::settings_changed(app);
}

/// Стать виджетом в панели задач на месте курсора (x, y).
#[cfg(windows)]
fn dock(app: &AppHandle, x: i32, y: i32) {
    use crate::taskbar;
    let order = taskbar::order_for_drop(x, y);
    let r = app.state::<SettingsStore>().update(|s| {
        s.view = "taskbar".into();
        s.taskbar_order = Some(order);
    });
    if let Err(e) = r {
        applog::warn(e);
        return;
    }
    taskbar::set_order(order);
    applog::info(format!("панель принесли в панель задач — виджет, порядок {order}"));
    let anim = taskbar::animations_on();
    let mut hwnd = 0;
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        hwnd = w.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        // Залёт (B10): панель втягивается в ближайшую к панели задач точку под курсором и
        // гаснет; потом прячем окно, а виджет появляется «шлепком». Без анимаций — просто
        // гаснет (иначе Windows на кадр показывает старую картинку — «силуэт»).
        let p = w.outer_position().unwrap_or_default();
        let size = w.outer_size().unwrap_or_default();
        let (tx, ty) = match taskbar::taskbar_rect() {
            Some(tb) if tb.h > tb.w => (tb.x + tb.w / 2, y),
            Some(tb) => (x, tb.y + tb.h / 2),
            None => (x, y),
        };
        let ox = (tx - p.x).clamp(0, size.width as i32);
        let oy = (ty - p.y).clamp(0, size.height as i32);
        let _ = tauri::Emitter::emit_to(app, MAIN_LABEL, "panel-fly-in", serde_json::json!({ "x": ox, "y": oy, "anim": anim }));
    }
    std::thread::sleep(Duration::from_millis(if anim { FLY_IN_MS } else { 24 }));
    apply(app);
    // Системное перетаскивание окна (если шло) — прервать, но только когда окно уже
    // спрятано: при отмене Windows возвращает окно туда, где его схватили, и раньше оно
    // мелькало в стартовой точке (Рома, 30.10)
    if hwnd != 0 {
        taskbar::cancel_mode(hwnd);
    }
    shell::settings_changed(app);
}

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
    fn dock_zone() {
        let tb = Rect { x: 0, y: 1032, w: 1920, h: 48 };
        assert!(near(tb, 500, 1040, 0));
        assert!(near(tb, 500, 1000, 36), "чуть выше панели — уже зона");
        assert!(!near(tb, 500, 990, 36));
        assert!(!near(tb, 500, 960, 36));
    }

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
