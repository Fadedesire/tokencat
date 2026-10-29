// Вид «Поверх окон» (S07): главное окно — маленькая панель без рамки, как в прототипе.
//
// - Прозрачное окно без рамки и тени, скругления рисует CSS панели.
// - Поверх всех окон (переключается в меню, сохраняется в settings.json).
// - Нет в панели задач и в Alt+Tab (Windows: стиль WS_EX_TOOLWINDOW).
// - Перетаскивание и двойной клик делает JS (main.js), меню ПКМ — нативное, общее с треем
//   (tray.rs), нажатия обрабатывает shell.rs.
// - До конца онбординга (B1) панель не показываем.
// - У дополнительных профилей место по умолчанию сдвинуто вниз, чтобы панели двух
//   аккаунтов не легли друг на друга.
// - Позиция запоминается (с задержкой, чтобы не писать файл на каждый пиксель) и при старте
//   проверяется: если такого места на экранах больше нет (монитор отключили, сменили
//   разрешение), панель встаёт в правый верхний угол основного монитора.
// - Размер окна подгоняется под панель: JS сообщает её размер в CSS-пикселях
//   (логических), Tauri сам переводит в физические при любом масштабе 100/125/150 %.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

use crate::auth::{only_main, MAIN_LABEL};
use crate::profile::Profile;
use crate::settings::SettingsStore;
use crate::{applog, tray};

type CmdResult<T> = Result<T, String>;

/// Ширина панели по умолчанию (как --tc-w в tokens.css), логические пиксели
const PANEL_W: f64 = 252.0;
/// Отступ от краёв экрана для позиции по умолчанию, логические пиксели
const EDGE: f64 = 16.0;
/// Сдвиг вниз для каждого следующего профиля, логические пиксели (панель ~110 px + зазор)
const SLOT_STEP: f64 = 130.0;

/// Прямоугольник в физических пикселях виртуального экрана: x, y, ширина, высота.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// Можно ли оставить панель в этом месте: за неё должно быть можно схватиться.
/// Проверяем три точки шапки (левый край, середина, правый край, 10 пикселей от верха):
/// хотя бы середина и одна из боковых должны быть на каком-нибудь мониторе.
pub fn grabbable(win: Rect, monitors: &[Rect]) -> bool {
    let y = win.y + 10.min(win.h / 2);
    let on = |x: i32| monitors.iter().any(|m| m.contains(x, y));
    let mid = on(win.x + win.w / 2);
    let left = on(win.x + 20.min(win.w / 4));
    let right = on(win.x + win.w - 20.min(win.w / 4));
    mid && (left || right)
}

/// Правый верхний угол монитора с отступом `edge` (физические пиксели).
pub fn default_spot(mon: Rect, win_w: i32, edge: i32) -> (i32, i32) {
    let x = (mon.x + mon.w - win_w - edge).max(mon.x);
    (x, mon.y + edge)
}

fn monitor_rect(m: &tauri::Monitor) -> Rect {
    let p = m.position();
    let s = m.size();
    Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }
}

/// Поставить панель на сохранённое место или в угол основного монитора.
pub fn place(w: &WebviewWindow, app: &AppHandle) {
    let s = app.state::<SettingsStore>().get();
    let size = w.outer_size().unwrap_or_default();
    let scale = w.scale_factor().unwrap_or(1.0);
    let ww = if size.width > 0 { size.width as i32 } else { (PANEL_W * scale) as i32 };
    let wh = size.height.max(40) as i32;
    let monitors: Vec<Rect> = w
        .available_monitors()
        .map(|v| v.iter().map(monitor_rect).collect())
        .unwrap_or_default();

    if let (Some(x), Some(y)) = (s.overlay_x, s.overlay_y) {
        if monitors.is_empty() || grabbable(Rect { x, y, w: ww, h: wh }, &monitors) {
            let _ = w.set_position(PhysicalPosition::new(x, y));
            return;
        }
        applog::info(format!("панель была за пределами экранов ({x}, {y}) — ставлю в угол"));
    }
    let primary = w.primary_monitor().ok().flatten();
    let (mon, mscale) = match &primary {
        Some(m) => (monitor_rect(m), m.scale_factor()),
        None => match monitors.first() {
            Some(r) => (*r, scale),
            None => return, // мониторов не знаем — пусть стоит, где поставила система
        },
    };
    let (x, y) = default_spot(mon, ww, (EDGE * mscale) as i32);
    let slot = app.state::<Profile>().slot();
    let y = slot_y(mon, y, slot, (SLOT_STEP * mscale) as i32);
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

/// Вертикаль для копии номер `slot`: ниже на `step` за каждый слот, но не ниже середины экрана
/// (на маленьком экране панели лягут внахлёст, но обе останутся видны).
pub fn slot_y(mon: Rect, y: i32, slot: u32, step: i32) -> i32 {
    let max = mon.y + mon.h / 2;
    (y + step * slot as i32).min(max.max(y))
}

/// Панель можно показывать: онбординг пройден и выбран вид «Поверх окон».
pub fn panel_allowed(app: &AppHandle) -> bool {
    let s = app.state::<SettingsStore>().get();
    s.onboarded && s.view == "overlay"
}

/// Windows: убрать окно из Alt+Tab. skipTaskbar прячет только кнопку на панели задач,
/// а Alt+Tab показывает все окна без стиля «окно инструментов».
#[cfg(windows)]
fn hide_from_alt_tab(w: &WebviewWindow) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let Ok(h) = w.hwnd() else { return };
    let hwnd = h.0 as HWND;
    // SAFETY: hwnd — живое окно этого процесса; меняем только расширенный стиль.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = (ex | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

#[cfg(not(windows))]
fn hide_from_alt_tab(_w: &WebviewWindow) {}

/// Вызывается из setup: настроить главное окно до показа.
pub fn setup(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MAIN_LABEL) else { return };
    hide_from_alt_tab(&w);
    let on_top = app.state::<SettingsStore>().get().always_on_top;
    let _ = w.set_always_on_top(on_top);
    let _ = w.set_size(LogicalSize::new(PANEL_W, 80.0));
    place(&w, app);
    // Окно показывает overlay_fit, когда панель уже нарисована (без мигания пустого окна).
    // Страховка: если JS не ответил, всё равно показать через 3 с.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if !panel_allowed(&app2) {
            return;
        }
        if let Some(w) = app2.get_webview_window(MAIN_LABEL) {
            if !w.is_visible().unwrap_or(true) {
                applog::info("панель не сообщила размер за 3 с — показываю как есть");
                let _ = w.show();
            }
        }
    });
}

// ---------- Позиция ----------

static MOVE_GEN: AtomicU64 = AtomicU64::new(0);

/// Окно передвинули. Сохраняем, когда движение затихло на 0,7 с.
pub fn on_moved(app: &AppHandle, pos: PhysicalPosition<i32>) {
    // Свёрнутые окна Windows уезжают в (-32000, -32000) — это не позиция
    if pos.x <= -30000 || pos.y <= -30000 {
        return;
    }
    // Всплывающая панель (виды B2) стоит у виджета — это не место плавающей панели
    if crate::views::is_popup(app) {
        return;
    }
    let gen = MOVE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        if MOVE_GEN.load(Ordering::SeqCst) != gen {
            return; // ещё двигают
        }
        let r = app.state::<SettingsStore>().update(|s| {
            s.overlay_x = Some(pos.x);
            s.overlay_y = Some(pos.y);
        });
        if let Err(e) = r {
            applog::warn(format!("не удалось сохранить позицию панели: {e}"));
        }
    });
}

// ---------- Команды ----------

/// Панель сообщила свой размер (CSS-пиксели). Подгоняем окно и показываем его.
#[tauri::command]
pub fn overlay_fit(window: WebviewWindow, app: AppHandle, width: f64, height: f64) -> CmdResult<()> {
    if window.label() != MAIN_LABEL {
        return Err("forbidden".into());
    }
    let w = width.clamp(120.0, 600.0);
    let h = height.clamp(30.0, 600.0);
    window.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    // Всплывающая панель выросла/уменьшилась — снова прижать к виджету
    if crate::views::is_popup(&app) {
        if window.is_visible().unwrap_or(false) {
            crate::views::place_popup(&app);
        }
        return Ok(());
    }
    if panel_allowed(&app) && !window.is_visible().unwrap_or(true) {
        window.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Меню по правой кнопке мыши — то же, что у значка в трее.
#[tauri::command]
pub async fn overlay_menu(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    only_main(&window)?;
    let menu = tray::build_menu(&app).map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Два монитора 1920×1080: основной слева, второй справа
    const A: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    const B: Rect = Rect { x: 1920, y: 0, w: 1920, h: 1080 };

    #[test]
    fn inside_and_across_monitors() {
        let win = |x, y| Rect { x, y, w: 315, h: 120 };
        assert!(grabbable(win(100, 100), &[A]));
        assert!(grabbable(win(1800, 100), &[A, B])); // на стыке двух мониторов
        assert!(grabbable(win(3000, 500), &[A, B]));
        // Второй монитор отключили — панель осталась «в пустоте»
        assert!(!grabbable(win(3000, 500), &[A]));
        // Почти целиком за правым краем: видна только левая полоска
        assert!(!grabbable(win(1900, 100), &[A]));
        // Шапка выше экрана — не схватить
        assert!(!grabbable(win(100, -50), &[A]));
        // Немного вылезает влево — ещё можно схватить
        assert!(grabbable(win(-60, 100), &[A]));
    }

    #[test]
    fn monitor_left_or_above_primary() {
        let left = Rect { x: -2560, y: -200, w: 2560, h: 1440 };
        assert!(grabbable(Rect { x: -1000, y: -150, w: 378, h: 140 }, &[A, left]));
        assert!(!grabbable(Rect { x: -1000, y: -150, w: 378, h: 140 }, &[A]));
    }

    #[test]
    fn second_profile_lower() {
        assert_eq!(slot_y(A, 16, 0, 130), 16);
        assert_eq!(slot_y(A, 16, 1, 130), 146);
        assert_eq!(slot_y(A, 16, 2, 130), 276);
        // Не ниже середины экрана
        assert_eq!(slot_y(A, 16, 7, 130), 540);
        assert_eq!(slot_y(B, 16, 7, 130), 540);
    }

    #[test]
    fn default_spot_top_right() {
        assert_eq!(default_spot(A, 315, 20), (1920 - 315 - 20, 20));
        assert_eq!(default_spot(B, 315, 20), (3840 - 315 - 20, 20));
        // Экран уже окна — не уезжаем левее монитора
        assert_eq!(default_spot(Rect { x: 0, y: 0, w: 200, h: 600 }, 315, 20), (0, 20));
    }
}
