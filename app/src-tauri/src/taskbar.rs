#![cfg_attr(not(windows), allow(dead_code))]

// Вид «В панели задач» (B2, Windows): маленький виджет прямо в панели задач, слева от трея.
//
// Как устроено (решение в DECISIONS, B2):
// - Не WebView, а обычное окно Win32 с рисованием GDI: внутри панели задач (чужой процесс
//   explorer.exe) WebView2 тяжёл и капризен, а здесь нужны котик, две строки и две полоски.
// - Окно — дочернее для `Shell_TrayWnd` (сама панель задач), стоит левее `TrayNotifyWnd`
//   (область уведомлений). Прозрачный фон — «слоистое» окно (WS_EX_LAYERED; дочерним окнам
//   это разрешено только с манифестом «Windows 8+», см. build.rs и manifest.xml):
//   картинка с альфа-каналом через UpdateLayeredWindow, фон почти прозрачный (альфа 1/255) —
//   так весь прямоугольник виджета ловит клики. Цветовой ключ (LWA_COLORKEY) — запасной путь:
//   он пропускает клики сквозь прозрачные места. Совсем не вышло — сплошной фон.
// - Своё окно-«хозяин» (невидимое, верхнего уровня) живёт в отдельном потоке с циклом
//   сообщений: ловит `TaskbarCreated` (перезапуск Проводника), смену темы, таймер раз в
//   секунду (панель переехала, сменился масштаб, автоскрытие, ширина трея) и таймер котика.
// - Поток виджета связан с потоком Проводника (общая очередь ввода), поэтому в нём ничего не
//   ждём: данные берём из Mutex на мгновение, всё остальное отдаём в Tauri через spawn.
// - Встроиться не удалось 10 секунд подряд — сообщаем приложению, и оно включает запасной
//   вид: проценты рисуются прямо на значке в трее (views.rs).
//
// Расчёт размеров и места (чистые функции ниже) проверяется тестами на любой ОС.

use crate::icons::Mood;
use crate::overlay::Rect;

/// Что показать на виджете.
#[derive(Debug, Clone, PartialEq)]
pub struct Data {
    pub session: Option<f64>,
    pub weekly: Option<f64>,
    /// Данные устарели (нет сети/VPN) — бледный текст
    pub stale: bool,
    pub mood: Mood,
    pub cat: bool,
}

impl Default for Data {
    fn default() -> Self {
        Data { session: None, weekly: None, stale: false, mood: Mood::Sleep, cat: true }
    }
}

/// Событие от виджета в приложение.
pub enum Event {
    /// ЛКМ: показать/спрятать панель у виджета (прямоугольник виджета на экране)
    Click(Rect),
    /// ПКМ: меню в точке экрана
    Menu(i32, i32),
    /// Встроились (true) или перешли на запасной вид (false)
    Embedded(bool),
    /// Виджет вытянули из панели задач: стать плавающей панелью под курсором (x, y)
    DragOut(i32, i32),
    /// Виджет переставили в ряду: новый порядок (сохранить в настройках профиля)
    Reorder(i32),
    /// Вытянутый виджет ведёт плавающую панель: поставить её левый верхний угол сюда
    FollowTo(i32, i32),
}

// ---------- Размеры (логические пиксели × масштаб) ----------

const PAD: f64 = 6.0;
const CAT_CELL: f64 = 2.0;
const LABEL_W: f64 = 16.0;
const BAR_W: f64 = 34.0;
const BAR_H: f64 = 4.0;
const PCT_W: f64 = 32.0;
const ROW_H: f64 = 15.0;
const FONT: f64 = 12.0;
/// Зазор между виджетом и треем
pub const GAP: f64 = 4.0;

/// Раскладка виджета в его собственных координатах (физические пиксели).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub w: i32,
    pub h: i32,
    pub vertical: bool,
    pub font_px: i32,
    /// Клетка котика и левый верхний угол спрайта; cell = 0 — без котика
    pub cat_cell: i32,
    pub cat_x: i32,
    pub cat_y: i32,
    /// Строки: x подписи, x полоски, ширина полоски, правый край процентов, середина строки
    pub label_x: i32,
    pub bar_x: i32,
    pub bar_w: i32,
    pub bar_h: i32,
    pub pct_right: i32,
    pub row_y: [i32; 2],
}

fn px(v: f64, scale: f64) -> i32 {
    (v * scale).round() as i32
}

/// Раскладка для толщины панели `thick` (высота горизонтальной / ширина вертикальной панели).
pub fn layout(scale: f64, thick: i32, vertical: bool, cat: bool) -> Layout {
    let cell = if cat { px(CAT_CELL, scale).max(1) } else { 0 };
    let cat_w = cell * crate::icons::CAT_W as i32;
    let cat_h = cell * crate::icons::CAT_H as i32;
    let pad = px(PAD, scale);
    let row_h = px(ROW_H, scale);
    let font_px = px(FONT, scale);
    if !vertical {
        // [котик] 5ч ▬▬▬ 42%
        //         Н  ▬▬▬ 18%
        let cat_x = pad;
        let label_x = if cat { cat_x + cat_w + pad } else { pad };
        let bar_x = label_x + px(LABEL_W, scale);
        let bar_w = px(BAR_W, scale);
        let pct_right = bar_x + bar_w + px(4.0, scale) + px(PCT_W, scale);
        let w = pct_right + pad;
        let mid = thick / 2;
        Layout {
            w,
            h: thick,
            vertical,
            font_px,
            cat_cell: cell,
            cat_x,
            cat_y: (thick - cat_h) / 2,
            label_x,
            bar_x,
            bar_w,
            bar_h: px(BAR_H, scale).max(2),
            pct_right,
            row_y: [mid - row_h / 2, mid + row_h / 2],
        }
    } else {
        // Вертикальная панель (Windows 10): котик сверху, под ним «42%» и «18%» с полосками
        let top = if cat { pad + cat_h } else { 0 };
        let h = top + pad + 2 * (row_h + px(BAR_H, scale) + px(2.0, scale)) + pad;
        let bar_w = (thick - 2 * pad).max(8);
        Layout {
            w: thick,
            h,
            vertical,
            font_px,
            cat_cell: cell,
            cat_x: (thick - cat_w) / 2,
            cat_y: pad,
            label_x: pad,
            bar_x: pad,
            bar_w,
            bar_h: px(BAR_H, scale).max(2),
            pct_right: thick - pad,
            row_y: [top + pad + row_h / 2, top + pad + row_h + px(BAR_H, scale) + px(2.0, scale) + row_h / 2],
        }
    }
}

/// Где поставить виджет внутри панели задач (координаты относительно панели).
/// `tb` — панель на экране, `tray` — область уведомлений (None — не нашли).
pub fn spot(tb: Rect, tray: Option<Rect>, w: i32, h: i32, gap: i32) -> (i32, i32) {
    let vertical = tb.h > tb.w;
    match (vertical, tray.filter(|t| t.w > 0 && t.h > 0)) {
        (false, Some(t)) => ((t.x - tb.x - w - gap).max(0), 0),
        (true, Some(t)) => (0, (t.y - tb.y - h - gap).max(0)),
        // Трей не нашли: примерно там же, где он бывает (правая четверть / низ)
        (false, None) => ((tb.w * 3 / 4 - w).max(0), 0),
        (true, None) => (0, (tb.h * 3 / 4 - h).max(0)),
    }
}

/// Место виджета в ряду (B2): `order` — порядок (меньше — ближе к трею; по умолчанию
/// номер профиля × 100, меняется перетаскиванием), `slot` — номер профиля (при равном порядке).
pub type Key = (i32, u32);

/// Порядок по умолчанию: основной у самого трея, следующие профили — левее.
pub fn default_order(slot: u32) -> i32 {
    slot as i32 * 100
}

/// Сдвиг виджета от трея: сумма размеров тех, кто в ряду ближе к трею.
/// `others` — (ключ, ширина или высота) виджетов других копий программы.
pub fn queue_offset(me: Key, others: &[(Key, i32)], gap: i32) -> i32 {
    others.iter().filter(|(k, _)| *k < me).map(|(_, size)| size + gap).sum()
}

/// Новый порядок для виджета, отпущенного в точке `cursor` (x для горизонтальной панели,
/// y для вертикальной). Трей — со стороны больших координат, поэтому все, чей центр дальше
/// курсора по направлению к трею, остаются ближе к трею. `others` — (порядок, центр).
pub fn order_at(cursor: i32, others: &[(i32, i32)]) -> i32 {
    let mut v: Vec<(i32, i32)> = others.to_vec();
    v.sort();
    let k = v.iter().filter(|(_, c)| *c > cursor).count();
    match (k, v.len()) {
        (_, 0) => 0,
        (0, _) => v[0].0 - 100,
        (k, n) if k >= n => v[n - 1].0 + 100,
        (k, _) => {
            // Между соседями: k ближайших к трею по порядку — это первые k
            let (a, b) = (v[k - 1].0, v[k].0);
            if b - a >= 2 { a + (b - a) / 2 } else { a + 1 }
        }
    }
}

/// Подпись окна виджета: по ней копии программы находят друг друга в панели задач.
pub fn widget_title(k: Key) -> String {
    format!("TokenCat:{}:{}", k.0, k.1)
}

pub fn parse_widget_title(t: &str) -> Option<Key> {
    let rest = t.strip_prefix("TokenCat:")?;
    let (o, s) = rest.split_once(':')?;
    Some((o.parse().ok()?, s.parse().ok()?))
}

/// Текст процента: «42%», нет данных — «—».
pub fn pct_text(p: Option<f64>) -> String {
    match p {
        Some(v) => format!("{}%", v.round().clamp(0.0, 999.0) as i64),
        None => "—".into(),
    }
}

// ---------- Анимации (B10) ----------

/// «Шлепок» виджета при появлении, мс
pub const SPLAT_MS: f64 = 300.0;
/// Переезд виджета на новое место в ряду, мс
pub const SLIDE_MS: f64 = 140.0;

/// «Шлепок» (squash & stretch): момент t (0..1) → масштаб картинки по x и по y.
/// Узкий и вытянутый вверх → сплющенный → чуть вытянутый → норма.
pub fn splat(t: f64) -> (f64, f64) {
    const KEYS: [(f64, f64, f64); 4] = [(0.0, 0.45, 1.35), (0.4, 1.16, 0.8), (0.7, 0.96, 1.05), (1.0, 1.0, 1.0)];
    if t >= 1.0 {
        return (1.0, 1.0);
    }
    let t = t.max(0.0);
    for k in KEYS.windows(2) {
        let (a, b) = (k[0], k[1]);
        if t <= b.0 {
            let u = (t - a.0) / (b.0 - a.0);
            let u = u * u * (3.0 - 2.0 * u); // плавно в начале и в конце отрезка
            return (a.1 + (b.1 - a.1) * u, a.2 + (b.2 - a.2) * u);
        }
    }
    (1.0, 1.0)
}

/// Полоски заполняются волной слева направо, вторая строка чуть позже первой:
/// какая доля заполнения видна у строки `row` в момент t (0..1).
pub fn fill_wave(t: f64, row: usize) -> f64 {
    let start = 0.2 + 0.15 * row as f64;
    ease_out(((t - start) / 0.55).clamp(0.0, 1.0))
}

/// Плавное замедление к концу (cubic ease-out), t 0..1.
pub fn ease_out(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

#[cfg(windows)]
pub use win::{
    animations_on, cancel_mode, cursor_pos, left_button_down, order_for_drop, rect as widget_rect, set_enabled,
    set_order, start, taskbar_rect, update,
};

#[cfg(windows)]
mod win {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
        CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, GetTextExtentPoint32W,
        InvalidateRect, SelectObject, SetBkMode, SetTextColor, TextOutW, ANTIALIASED_QUALITY,
        FW_NORMAL, LOGFONTW, PAINTSTRUCT, SRCCOPY, TRANSPARENT, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, BLENDFUNCTION, CreateDIBSection, DIB_RGB_COLORS, GdiFlush, GetDC, ReleaseDC,
        AC_SRC_ALPHA, AC_SRC_OVER,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowExW, FindWindowW,
        GetCursorPos, GetMessageW, GetParent, GetWindowRect, GetWindowTextW, IsWindow, IsWindowVisible,
        KillTimer,
        LoadCursorW,
        PostMessageW, RegisterClassExW, RegisterWindowMessageW, SetLayeredWindowAttributes, SetTimer,
        UpdateLayeredWindow, ULW_ALPHA,
        SetWindowPos, TranslateMessage, HWND_TOP, IDC_ARROW, LWA_COLORKEY, MSG, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WM_APP, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONUP,
        WM_PAINT, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMER, WNDCLASSEXW,
        WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
        SetWindowTextW, WM_CANCELMODE, WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_MOUSEMOVE, ShowWindow, SW_HIDE,
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,

    };

    use super::{
        ease_out, fill_wave, layout, order_at, parse_widget_title, pct_text, queue_offset, splat, spot, widget_title,
        Data, Event, Key, Layout, GAP, SLIDE_MS, SPLAT_MS,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, ReleaseCapture, SetCapture, VK_LBUTTON};
    use crate::applog;
    use crate::icons::{self, cat_color, cat_frame, CatFrame, Mood, CAT_H, CAT_W};
    use crate::overlay::Rect;

    const WM_APP_ENABLE: u32 = WM_APP + 1;
    const WM_APP_DATA: u32 = WM_APP + 2;
    /// Порядок в ряду сменился: обновить подпись, встать на место, толкнуть соседей
    const WM_APP_ORDER: u32 = WM_APP + 3;
    const TIMER_LAYOUT: usize = 1;
    const TIMER_CAT: usize = 2;
    /// Кадры анимаций (B10), ~60 к/с; работает только пока идёт анимация
    const TIMER_ANIM: usize = 3;
    /// Сколько секунд подряд без панели задач — и переходим на запасной вид
    const FAIL_LIMIT: u32 = 10;
    const MODE_SOLID: u8 = 0;
    const MODE_KEY: u8 = 1;
    const MODE_ALPHA: u8 = 2;

    struct Shared {
        enabled: bool,
        data: Data,
    }

    static SHARED: Mutex<Shared> = Mutex::new(Shared {
        enabled: false,
        data: Data { session: None, weekly: None, stale: false, mood: Mood::Sleep, cat: true },
    });
    static CALLBACK: OnceLock<Box<dyn Fn(Event) + Send + Sync>> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);
    static HOST: AtomicIsize = AtomicIsize::new(0);
    static WIDGET: AtomicIsize = AtomicIsize::new(0);
    /// Номер профиля этой копии (0 — основной) и место в ряду виджетов (см. Key)
    static SLOT: AtomicU32 = AtomicU32::new(0);
    static ORDER: AtomicI32 = AtomicI32::new(0);
    /// Общее сообщение всех копий TokenCat «ряд в панели задач изменился — встань на место»
    /// (RegisterWindowMessage: одно число для всех процессов)
    static REFLOW_MSG: AtomicU32 = AtomicU32::new(0);
    /// Виджет вытянули: окно плавающей панели, которое он ведёт за курсором, и за какую
    /// точку панель «держат» (см. follow)
    static FOLLOW: AtomicIsize = AtomicIsize::new(0);
    static FOLLOW_GX: AtomicI32 = AtomicI32::new(0);
    static FOLLOW_GY: AtomicI32 = AtomicI32::new(0);

    fn my_key() -> super::Key {
        (ORDER.load(Ordering::SeqCst), SLOT.load(Ordering::SeqCst))
    }

    /// Состояние, которое трогает только поток виджета (поэтому Cell, без блокировок:
    /// оконные процедуры вызываются повторно изнутри Win32-вызовов).
    struct Local {
        taskbar: Cell<isize>,
        dpi: Cell<u32>,
        place: Cell<(i32, i32, i32, i32)>,
        light: Cell<bool>,
        /// Как рисуем: MODE_SOLID, MODE_KEY (цветовой ключ) или MODE_ALPHA (UpdateLayeredWindow)
        mode: Cell<u8>,
        fails: Cell<u32>,
        /// None — ещё не сообщали, Some(true/false) — встроены / запасной вид
        embedded: Cell<Option<bool>>,
        tick: Cell<u64>,
        frame: Cell<CatFrame>,
        taskbar_created: Cell<u32>,
        /// Нажали ЛКМ на виджете: точка экрана и смещение курсора внутри виджета
        press: Cell<Option<(i32, i32, i32, i32)>>,
        /// Виджет тащат вдоль панели (перестановка)
        dragging: Cell<bool>,
        /// Виджет вытянули, кнопка ещё нажата: ведём плавающую панель (захват мыши у виджета)
        following: Cell<bool>,
        /// Идёт «шлепок» появления (B10): когда начался
        splat: Cell<Option<Instant>>,
        /// Идёт переезд в ряду (B10): откуда (x, y), куда (x, y), когда начался
        slide: Cell<Option<(i32, i32, i32, i32, Instant)>>,
    }

    thread_local! {
        static L: Local = Local {
            taskbar: Cell::new(0),
            dpi: Cell::new(96),
            place: Cell::new((0, 0, 0, 0)),
            light: Cell::new(false),
            mode: Cell::new(MODE_ALPHA),
            fails: Cell::new(0),
            embedded: Cell::new(None),
            tick: Cell::new(0),
            frame: Cell::new([[0; CAT_W]; CAT_H]),
            taskbar_created: Cell::new(0),
            press: Cell::new(None),
            dragging: Cell::new(false),
            following: Cell::new(false),
            splat: Cell::new(None),
            slide: Cell::new(None),
        };
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn h(v: isize) -> HWND {
        v as HWND
    }

    const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
        r as u32 | (g as u32) << 8 | (b as u32) << 16
    }

    fn rgb3(c: [u8; 3]) -> COLORREF {
        rgb(c[0], c[1], c[2])
    }

    /// Запустить поток виджета (один раз). `cb` получает клики и смену состояния.
    pub fn start(slot: u32, order: i32, cb: impl Fn(Event) + Send + Sync + 'static) {
        SLOT.store(slot, Ordering::SeqCst);
        ORDER.store(order, Ordering::SeqCst);
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = CALLBACK.set(Box::new(cb));
        let r = std::thread::Builder::new().name("tokencat-taskbar".into()).spawn(thread_main);
        if let Err(e) = r {
            applog::warn(format!("панель задач: не удалось запустить поток: {e}"));
            emit(Event::Embedded(false));
        }
    }

    fn emit(e: Event) {
        if let Some(cb) = CALLBACK.get() {
            cb(e);
        }
    }

    /// Включить/выключить виджет (смена вида).
    pub fn set_enabled(on: bool) {
        SHARED.lock().unwrap().enabled = on;
        let host = HOST.load(Ordering::SeqCst);
        if host != 0 {
            // SAFETY: окно-хозяин живёт до конца процесса; PostMessage не ждёт.
            unsafe { PostMessageW(h(host), WM_APP_ENABLE, 0, 0) };
        }
    }

    /// Новые данные для виджета.
    pub fn update(d: Data) {
        {
            let mut s = SHARED.lock().unwrap();
            if s.data == d {
                return;
            }
            s.data = d;
        }
        let host = HOST.load(Ordering::SeqCst);
        if host != 0 {
            // SAFETY: см. set_enabled
            unsafe { PostMessageW(h(host), WM_APP_DATA, 0, 0) };
        }
    }

    /// Прямоугольник виджета на экране, если он сейчас встроен.
    pub fn rect() -> Option<Rect> {
        let w = WIDGET.load(Ordering::SeqCst);
        if w == 0 {
            return None;
        }
        let mut r: RECT = unsafe { std::mem::zeroed() };
        // SAFETY: GetWindowRect только читает; для мёртвого окна вернёт 0
        if unsafe { GetWindowRect(h(w), &mut r) } == 0 {
            return None;
        }
        Some(Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top })
    }

    fn data() -> Data {
        SHARED.lock().unwrap().data.clone()
    }

    fn enabled() -> bool {
        SHARED.lock().unwrap().enabled
    }

    // ---------- Поток ----------

    fn thread_main() {
        // SAFETY: классические вызовы Win32 в собственном потоке; все строки живут до конца
        // функции (классы регистрируются один раз, имена — в static ниже).
        unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let host_class = wide("TokenCatTaskbarHost");
            let widget_class = wide("TokenCatTaskbarWidget");
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(host_proc);
            wc.hInstance = hinst;
            wc.lpszClassName = host_class.as_ptr();
            RegisterClassExW(&wc);
            let mut wc2: WNDCLASSEXW = std::mem::zeroed();
            wc2.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc2.lpfnWndProc = Some(widget_proc);
            wc2.hInstance = hinst;
            wc2.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            wc2.lpszClassName = widget_class.as_ptr();
            RegisterClassExW(&wc2);
            WIDGET_CLASS.with(|c| c.set(widget_class.as_ptr() as isize));

            let msg = wide("TaskbarCreated");
            let tc = RegisterWindowMessageW(msg.as_ptr());
            L.with(|l| l.taskbar_created.set(tc));
            let rm = wide("TokenCatTaskbarReflow");
            REFLOW_MSG.store(RegisterWindowMessageW(rm.as_ptr()), Ordering::SeqCst);

            // Невидимое окно верхнего уровня: только оно получает TaskbarCreated и смену темы
            let title = wide("TokenCat taskbar host");
            let host = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                host_class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            );
            if host.is_null() {
                applog::warn("панель задач: не удалось создать окно-хозяина");
                emit(Event::Embedded(false));
                return;
            }
            HOST.store(host as isize, Ordering::SeqCst);
            L.with(|l| l.light.set(read_light_theme()));
            apply_enabled(host);

            let mut m: MSG = std::mem::zeroed();
            while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
    }

    thread_local! {
        static WIDGET_CLASS: Cell<isize> = const { Cell::new(0) };
    }

    /// Включили — создать виджет и таймеры; выключили — убрать.
    unsafe fn apply_enabled(host: HWND) {
        if enabled() {
            // Панель, которую только что вытянули, принесли обратно — ведение заканчиваем
            stop_follow();
            L.with(|l| l.fails.set(0));
            SetTimer(host, TIMER_LAYOUT, 1000, None);
            SetTimer(host, TIMER_CAT, 125, None);
            ensure_widget();
        } else {
            KillTimer(host, TIMER_LAYOUT);
            KillTimer(host, TIMER_CAT);
            if L.with(|l| l.following.get()) {
                // Виджет ещё держит мышь и ведёт панель: прячем, уберём, когда отпустят
                let w = WIDGET.load(Ordering::SeqCst);
                if w != 0 {
                    ShowWindow(h(w), SW_HIDE);
                    nudge_neighbours();
                }
            } else {
                destroy_widget();
            }
            L.with(|l| l.embedded.set(None));
        }
    }

    /// Закончить ведение панели (кнопку отпустили или панель вернули в панель задач).
    unsafe fn stop_follow() {
        if !L.with(|l| l.following.replace(false)) {
            return;
        }
        FOLLOW.store(0, Ordering::SeqCst);
        ReleaseCapture();
        // Спрятанный виджет больше не нужен; если вид снова «панель задач» — ensure_widget
        // создаст новый на нужном месте ряда
        destroy_widget();
    }

    /// Прервать системное перетаскивание окна (панель «влетела» в панель задач, дальше её
    /// ведёт приложение).
    pub fn cancel_mode(hwnd: isize) {
        // SAFETY: PostMessage не ждёт; окно — наше
        unsafe { PostMessageW(h(hwnd), WM_CANCELMODE, 0, 0) };
    }

    /// Приложение показало плавающую панель для вытянутого виджета: дальше виджет сам двигает
    /// её за курсором на каждое движение мыши (захват мыши — у него). Системное
    /// перетаскивание тут не годится: кнопку зажали в панели задач, в чужом потоке ввода.
    #[allow(dead_code)] // ведение панели теперь в views::watch_drag
    pub fn follow(panel: isize, gx: i32, gy: i32) {
        FOLLOW_GX.store(gx, Ordering::SeqCst);
        FOLLOW_GY.store(gy, Ordering::SeqCst);
        FOLLOW.store(panel, Ordering::SeqCst);
    }

    /// Ведомая панель — под курсор. Само окно двигает приложение (Event::FollowTo): оно
    /// держит в очереди не больше одного переезда и всегда берёт последнюю точку. Если слать
    /// SetWindowPos на каждое движение мыши (их сотни в секунду), переезды копятся, и панель
    /// повторяет путь курсора с большим опозданием.
    unsafe fn follow_move() {
        if FOLLOW.load(Ordering::SeqCst) == 0 {
            return;
        }
        let (x, y) = cursor_pos();
        let (gx, gy) = (FOLLOW_GX.load(Ordering::SeqCst), FOLLOW_GY.load(Ordering::SeqCst));
        emit(Event::FollowTo(x - gx, y - gy));
    }

    unsafe fn destroy_widget() {
        let w = WIDGET.swap(0, Ordering::SeqCst);
        if w != 0 {
            DestroyWindow(h(w));
            nudge_neighbours();
        }
    }

    fn report(embedded: bool) {
        let changed = L.with(|l| {
            let was = l.embedded.get();
            l.embedded.set(Some(embedded));
            was != Some(embedded)
        });
        if changed {
            applog::info(if embedded {
                "панель задач: виджет встроен"
            } else {
                "панель задач: встроиться не удалось — показываю проценты на значке в трее"
            });
            emit(Event::Embedded(embedded));
        }
    }

    fn win_rect(w: HWND) -> Option<Rect> {
        let mut r: RECT = unsafe { std::mem::zeroed() };
        // SAFETY: только чтение
        if w.is_null() || unsafe { GetWindowRect(w, &mut r) } == 0 {
            return None;
        }
        Some(Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top })
    }

    /// Найти панель задач, создать или передвинуть виджет. Раз в секунду и по событиям.
    unsafe fn ensure_widget() {
        let cls = wide("Shell_TrayWnd");
        let tb = FindWindowW(cls.as_ptr(), std::ptr::null());
        let Some(tbr) = win_rect(tb) else {
            // Проводник перезапускается или панели нет вовсе
            destroy_widget();
            let fails = L.with(|l| {
                l.fails.set(l.fails.get() + 1);
                l.fails.get()
            });
            if fails >= FAIL_LIMIT {
                report(false);
            }
            return;
        };
        let tray_cls = wide("TrayNotifyWnd");
        let tray = win_rect(FindWindowExW(tb, std::ptr::null_mut(), tray_cls.as_ptr(), std::ptr::null()));

        // Старый виджет у старой панели (Проводник перезапустился) — убрать
        let mut w = h(WIDGET.load(Ordering::SeqCst));
        if !w.is_null() && (IsWindow(w) == 0 || GetParent(w) != tb) {
            destroy_widget();
            w = std::ptr::null_mut();
        }

        let dpi = match GetDpiForWindow(tb) {
            0 => 96,
            d => d,
        };
        let scale = dpi as f64 / 96.0;
        let vertical = tbr.h > tbr.w;
        let thick = if vertical { tbr.w } else { tbr.h };
        let lay = layout(scale, thick, vertical, data().cat);
        let gap = (GAP * scale).round() as i32;
        let (x, y) = spot(tbr, tray, lay.w, lay.h, gap);
        // Виджеты других профилей в этой же панели — встаём в очередь за ними
        let others: Vec<(Key, i32)> =
            neighbours(tb, w).into_iter().map(|(k, r)| (k, if vertical { r.h } else { r.w })).collect();
        let shift = queue_offset(my_key(), &others, gap);
        let (x, y) = if vertical { (x, (y - shift).max(0)) } else { ((x - shift).max(0), y) };

        if w.is_null() {
            // Масштаб нужен уже для первой картинки
            L.with(|l| l.dpi.set(dpi));
            // Появление — «шлепком» (B10): первая же картинка уже сжата
            let anim = animations_on();
            L.with(|l| {
                l.splat.set(if anim { Some(Instant::now()) } else { None });
                l.slide.set(None);
            });
            w = create_widget(tb, x, y, lay.w, lay.h);
            if anim && !w.is_null() {
                anim_start();
            }
            if w.is_null() {
                let fails = L.with(|l| {
                    l.fails.set(l.fails.get() + 1);
                    l.fails.get()
                });
                if fails >= FAIL_LIMIT {
                    report(false);
                }
                return;
            }
            L.with(|l| {
                l.taskbar.set(tb as isize);
                l.place.set((x, y, lay.w, lay.h));
                l.dpi.set(dpi);
            });
            if tray.is_none() {
                applog::warn("панель задач: область уведомлений не найдена — ставлю виджет примерно");
            }
        } else if L.with(|l| l.dragging.get()) {
            // Виджет сейчас тащат — место задаёт мышь
        } else {
            let (moved, old_w) = L.with(|l| {
                let old = l.place.get();
                let changed = old != (x, y, lay.w, lay.h) || l.dpi.get() != dpi;
                l.place.set((x, y, lay.w, lay.h));
                l.dpi.set(dpi);
                (changed, old.2)
            });
            if moved {
                // Место в ряду сменилось, размер тот же — плавный переезд (B10), иначе сразу
                let from = win_rect(w)
                    .filter(|r| r.w == lay.w && r.h == lay.h)
                    .map(|r| (r.x - tbr.x, r.y - tbr.y))
                    .filter(|&f| f != (x, y) && animations_on());
                match from {
                    Some((fx, fy)) => {
                        L.with(|l| l.slide.set(Some((fx, fy, x, y, Instant::now()))));
                        anim_start();
                    }
                    None => {
                        L.with(|l| l.slide.set(None));
                        SetWindowPos(w, HWND_TOP, x, y, lay.w, lay.h, SWP_NOACTIVATE);
                    }
                }
                redraw(w);
                // Сменилась ширина (котик, масштаб) — соседям тоже пора сдвинуться
                if old_w != lay.w {
                    nudge_neighbours();
                }
            } else {
                // Панель задач Windows 11 иногда перекладывает свои слои поверх нас
                SetWindowPos(w, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        L.with(|l| l.fails.set(0));
        report(true);
    }

    /// Виджеты других копий TokenCat в панели задач: (ключ, прямоугольник на экране).
    /// Ищем по имени класса и читаем подпись — чужое окно при этом не опрашивается.
    unsafe fn neighbours(tb: HWND, me: HWND) -> Vec<(Key, Rect)> {
        let class = wide("TokenCatTaskbarWidget");
        let mut out = Vec::new();
        let mut cur: HWND = std::ptr::null_mut();
        loop {
            cur = FindWindowExW(tb, cur, class.as_ptr(), std::ptr::null());
            if cur.is_null() {
                break;
            }
            // Свой и спрятанные (вытянутый виджет, который ещё держит мышь) — не в ряду
            if cur == me || IsWindowVisible(cur) == 0 {
                continue;
            }
            let mut buf = [0u16; 48];
            let n = GetWindowTextW(cur, buf.as_mut_ptr(), buf.len() as i32);
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if let (Some(k), Some(r)) = (parse_widget_title(&title), win_rect(cur)) {
                out.push((k, r));
            }
        }
        out
    }

    fn find_taskbar() -> HWND {
        let cls = wide("Shell_TrayWnd");
        // SAFETY: только поиск окна
        unsafe { FindWindowW(cls.as_ptr(), std::ptr::null()) }
    }

    /// Панель задач на экране (основной монитор).
    pub fn taskbar_rect() -> Option<Rect> {
        win_rect(find_taskbar())
    }

    pub fn cursor_pos() -> (i32, i32) {
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: запись в локальную структуру
        unsafe { GetCursorPos(&mut p) };
        (p.x, p.y)
    }

    /// Включены ли «Эффекты анимации» Windows (Параметры → Специальные возможности →
    /// Визуальные эффекты). Выключены — всё без анимации.
    pub fn animations_on() -> bool {
        let mut on: i32 = 1;
        // SAFETY: запись в локальную переменную размера BOOL
        unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on as *mut i32 as *mut c_void, 0) };
        on != 0
    }

    /// Запустить таймер кадров анимации (если уже идёт — ничего).
    unsafe fn anim_start() {
        let host = HOST.load(Ordering::SeqCst);
        if host != 0 {
            SetTimer(h(host), TIMER_ANIM, 15, None);
        }
    }

    /// Кадр анимации: «шлепок» и переезд. Всё кончилось — таймер выключаем (в простое CPU 0).
    unsafe fn anim_tick(host: HWND) {
        let w = h(WIDGET.load(Ordering::SeqCst));
        let mut busy = false;
        if w.is_null() {
            L.with(|l| {
                l.splat.set(None);
                l.slide.set(None);
            });
        } else {
            if let Some(start) = L.with(|l| l.splat.get()) {
                if start.elapsed().as_secs_f64() * 1000.0 >= SPLAT_MS {
                    L.with(|l| l.splat.set(None)); // последний кадр — уже обычная картинка
                } else {
                    busy = true;
                }
                redraw(w);
            }
            if L.with(|l| l.dragging.get()) {
                L.with(|l| l.slide.set(None)); // виджет схватили — место задаёт мышь
            }
            if let Some((fx, fy, tx, ty, start)) = L.with(|l| l.slide.get()) {
                let t = start.elapsed().as_secs_f64() * 1000.0 / SLIDE_MS;
                let e = ease_out(t);
                let x = fx + ((tx - fx) as f64 * e).round() as i32;
                let y = fy + ((ty - fy) as f64 * e).round() as i32;
                SetWindowPos(w, HWND_TOP, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                if t >= 1.0 {
                    L.with(|l| l.slide.set(None));
                } else {
                    busy = true;
                }
            }
        }
        if !busy {
            KillTimer(host, TIMER_ANIM);
        }
    }

    /// Левая кнопка мыши сейчас нажата (панель ещё тащат).
    pub fn left_button_down() -> bool {
        // SAFETY: чтение состояния клавиши
        unsafe { GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000 != 0 }
    }

    /// Порядок для панели, которую бросили на панель задач в точке (x, y): встать между
    /// соседними виджетами там, куда бросили. Можно звать из любого потока.
    pub fn order_for_drop(x: i32, y: i32) -> i32 {
        let tb = find_taskbar();
        let Some(tbr) = win_rect(tb) else { return 0 };
        let vertical = tbr.h > tbr.w;
        let me = h(WIDGET.load(Ordering::SeqCst));
        // SAFETY: только поиск окон и чтение подписей
        let others: Vec<(i32, i32)> = unsafe { neighbours(tb, me) }
            .into_iter()
            .map(|((o, _), r)| (o, if vertical { r.y + r.h / 2 } else { r.x + r.w / 2 }))
            .collect();
        order_at(if vertical { y } else { x }, &others)
    }

    /// Новый порядок (после перетаскивания или из настроек).
    pub fn set_order(order: i32) {
        if ORDER.swap(order, Ordering::SeqCst) == order {
            return;
        }
        let host = HOST.load(Ordering::SeqCst);
        if host != 0 {
            // Подпись и место меняет поток виджета (окна его); PostMessage не ждёт
            // SAFETY: см. set_enabled
            unsafe { PostMessageW(h(host), WM_APP_ORDER, 0, 0) };
        }
    }

    /// Подпись виджета = его место в ряду (её читают соседи).
    unsafe fn update_title() {
        let w = WIDGET.load(Ordering::SeqCst);
        if w != 0 {
            let t = wide(&widget_title(my_key()));
            SetWindowTextW(h(w), t.as_ptr());
        }
    }

    /// Толкнуть виджеты других копий: пусть встанут на место сейчас, а не по таймеру.
    unsafe fn nudge_neighbours() {
        let msg = REFLOW_MSG.load(Ordering::SeqCst);
        let tb = find_taskbar();
        if msg == 0 || tb.is_null() {
            return;
        }
        let class = wide("TokenCatTaskbarWidget");
        let me = h(WIDGET.load(Ordering::SeqCst));
        let mut cur: HWND = std::ptr::null_mut();
        loop {
            cur = FindWindowExW(tb, cur, class.as_ptr(), std::ptr::null());
            if cur.is_null() {
                break;
            }
            if cur != me {
                PostMessageW(cur, msg, 0, 0);
            }
        }
    }

    unsafe fn create_widget(tb: HWND, x: i32, y: i32, w: i32, hh: i32) -> HWND {
        let title = wide(&widget_title(my_key()));
        let class = WIDGET_CLASS.with(|c| c.get()) as *const u16;
        let hinst = GetModuleHandleW(std::ptr::null());
        let mk = |ex: u32| {
            CreateWindowExW(
                ex,
                class,
                title.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                x,
                y,
                w,
                hh,
                tb,
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            )
        };
        let mut wnd = mk(WS_EX_LAYERED);
        let mut mode = MODE_ALPHA;
        if !wnd.is_null() {
            WIDGET.store(wnd as isize, Ordering::SeqCst);
            L.with(|l| l.mode.set(MODE_ALPHA));
            if !push_alpha(wnd) {
                mode = MODE_KEY;
                L.with(|l| l.mode.set(MODE_KEY));
                if SetLayeredWindowAttributes(wnd, key_color(), 0, LWA_COLORKEY) == 0 {
                    applog::warn("панель задач: прозрачный фон не включился — рисую сплошной");
                    WIDGET.store(0, Ordering::SeqCst);
                    DestroyWindow(wnd);
                    wnd = std::ptr::null_mut();
                }
            }
        }
        if wnd.is_null() {
            mode = MODE_SOLID;
            wnd = mk(0);
        }
        if wnd.is_null() {
            applog::warn("панель задач: не удалось создать окно виджета");
            return wnd;
        }
        L.with(|l| l.mode.set(mode));
        WIDGET.store(wnd as isize, Ordering::SeqCst);
        nudge_neighbours();
        SetWindowPos(wnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        applog::info(format!(
            "панель задач: виджет {w}×{hh} в ({x}, {y}), фон: {}",
            match mode {
                MODE_ALPHA => "прозрачный",
                MODE_KEY => "цветовой ключ",
                _ => "сплошной",
            }
        ));
        wnd
    }

    /// Перерисовать виджет: «слоистый» — сразу новой картинкой, остальные — через WM_PAINT.
    unsafe fn redraw(w: HWND) {
        if L.with(|l| l.mode.get()) == MODE_ALPHA {
            push_alpha(w);
        } else {
            InvalidateRect(w, std::ptr::null(), 0);
        }
    }

    // ---------- Тема ----------

    /// Светлая панель задач? (Параметры → Персонализация → Цвета → «Режим Windows»)
    fn read_light_theme() -> bool {
        let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let val = wide("SystemUsesLightTheme");
        let mut v: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: буфер на 4 байта, размер передан
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                val.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                &mut v as *mut u32 as *mut c_void,
                &mut size,
            )
        };
        r == 0 && v == 1
    }

    struct Palette {
        key: COLORREF,
        text: COLORREF,
        muted: COLORREF,
        track: COLORREF,
    }

    fn palette() -> Palette {
        if L.with(|l| l.light.get()) {
            Palette {
                key: rgb(0xee, 0xee, 0xef),
                text: rgb(0x1f, 0x1e, 0x1d),
                muted: rgb(0x6b, 0x69, 0x63),
                track: rgb(0xc4, 0xc2, 0xbc),
            }
        } else {
            Palette {
                key: rgb(0x1f, 0x1f, 0x20),
                text: rgb(0xf0, 0xee, 0xe6),
                muted: rgb(0x8f, 0x8c, 0x84),
                track: rgb(0x4a, 0x48, 0x44),
            }
        }
    }

    /// Цвет фона, который станет прозрачным. Близок к цвету панели — края сглаженного
    /// текста смешиваются с ним и не дают заметной каймы.
    fn key_color() -> COLORREF {
        palette().key
    }

    // ---------- Оконные процедуры ----------

    unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let tc = L.with(|l| l.taskbar_created.get());
        if msg != 0 && msg == tc {
            applog::info("панель задач пересоздана (перезапуск Проводника)");
            if enabled() {
                destroy_widget();
                ensure_widget();
            }
            return 0;
        }
        match msg {
            WM_APP_ENABLE => {
                apply_enabled(hwnd);
                0
            }
            WM_APP_ORDER => {
                update_title();
                if enabled() && !L.with(|l| l.dragging.get()) {
                    ensure_widget();
                }
                nudge_neighbours();
                0
            }
            WM_APP_DATA => {
                // Котика включили/выключили — меняется ширина; остальное — просто перерисовать
                if enabled() {
                    ensure_widget();
                }
                let w = WIDGET.load(Ordering::SeqCst);
                if w != 0 {
                    redraw(h(w));
                }
                0
            }
            WM_TIMER if wp == TIMER_LAYOUT => {
                if enabled() {
                    ensure_widget();
                }
                0
            }
            WM_TIMER if wp == TIMER_CAT => {
                cat_tick();
                0
            }
            WM_TIMER if wp == TIMER_ANIM => {
                anim_tick(hwnd);
                0
            }
            WM_SETTINGCHANGE | WM_THEMECHANGED => {
                let light = read_light_theme();
                if L.with(|l| l.light.replace(light)) != light {
                    applog::info(format!("панель задач: тема {}", if light { "светлая" } else { "тёмная" }));
                    // Цвет-ключ зависит от темы
                    let w = WIDGET.load(Ordering::SeqCst);
                    if w != 0 && L.with(|l| l.mode.get()) == MODE_KEY {
                        SetLayeredWindowAttributes(h(w), key_color(), 0, LWA_COLORKEY);
                    }
                    if w != 0 {
                        redraw(h(w));
                    }
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    /// Таймер котика: 8 кадров в секунду, перерисовка только при смене кадра.
    fn cat_tick() {
        let w = WIDGET.load(Ordering::SeqCst);
        let d = data();
        if w == 0 || !d.cat {
            return;
        }
        let t = L.with(|l| {
            l.tick.set(l.tick.get() + 1);
            l.tick.get()
        });
        let f = cat_frame(t, d.mood);
        if L.with(|l| l.frame.replace(f)) != f {
            // SAFETY: окно виджета нашего потока
            unsafe { redraw(h(w)) };
        }
    }

    unsafe extern "system" fn widget_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let reflow = REFLOW_MSG.load(Ordering::SeqCst);
        if reflow != 0 && msg == reflow {
            // Сосед сменил место / появился / пропал — встать на место сразу
            if enabled() && !L.with(|l| l.dragging.get()) {
                ensure_widget();
            }
            return 0;
        }
        match msg {
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_ERASEBKGND => 1,
            WM_LBUTTONDOWN => {
                if let Some(r) = win_rect(hwnd) {
                    let (x, y) = cursor_pos();
                    L.with(|l| {
                        l.press.set(Some((x, y, x - r.x, y - r.y)));
                        l.dragging.set(false);
                    });
                    SetCapture(hwnd);
                }
                0
            }
            WM_MOUSEMOVE => {
                if L.with(|l| l.following.get()) {
                    follow_move();
                } else if let Some((px, py, gx, gy)) = L.with(|l| l.press.get()) {
                    on_drag(hwnd, px, py, gx, gy);
                }
                0
            }
            WM_LBUTTONUP if L.with(|l| l.following.get()) => {
                follow_move();
                stop_follow();
                0
            }
            WM_LBUTTONUP => {
                let (press, dragging) = L.with(|l| (l.press.take(), l.dragging.replace(false)));
                if press.is_some() {
                    ReleaseCapture();
                    if dragging {
                        drop_in_row(hwnd);
                    } else if let Some(r) = win_rect(hwnd) {
                        emit(Event::Click(r));
                    }
                }
                0
            }
            WM_CAPTURECHANGED if L.with(|l| l.following.get()) => {
                L.with(|l| l.following.set(false));
                FOLLOW.store(0, Ordering::SeqCst);
                if !enabled() {
                    destroy_widget();
                }
                0
            }
            WM_CAPTURECHANGED => {
                // Захват мыши отняли (Alt+Tab и т. п.) — отменить перетаскивание
                let was = L.with(|l| {
                    l.press.set(None);
                    l.dragging.replace(false)
                });
                if was {
                    ensure_widget();
                }
                0
            }
            WM_RBUTTONUP => {
                let mut p = POINT { x: 0, y: 0 };
                GetCursorPos(&mut p);
                emit(Event::Menu(p.x, p.y));
                0
            }
            WM_DESTROY => {
                let _ = WIDGET.compare_exchange(hwnd as isize, 0, Ordering::SeqCst, Ordering::SeqCst);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    // ---------- Перетаскивание ----------

    /// Мышь двигается с нажатой кнопкой. Вдоль панели — виджет едет за курсором
    /// (перестановка), увели за пределы панели — превращаемся в плавающую панель.
    unsafe fn on_drag(hwnd: HWND, px: i32, py: i32, gx: i32, gy: i32) {
        let (x, y) = cursor_pos();
        let tb = GetParent(hwnd);
        let (Some(tbr), Some(r)) = (win_rect(tb), win_rect(hwnd)) else { return };
        let scale = L.with(|l| l.dpi.get()) as f64 / 96.0;
        let margin = (12.0 * scale) as i32;
        let outside = x < tbr.x - margin || y < tbr.y - margin || x > tbr.x + tbr.w + margin || y > tbr.y + tbr.h + margin;
        if outside {
            // Захват мыши не отпускаем: виджет будет вести появившуюся панель (follow)
            L.with(|l| {
                l.press.set(None);
                l.dragging.set(false);
                l.following.set(true);
            });
            emit(Event::DragOut(x, y));
            return;
        }
        let threshold = (4.0 * scale) as i32;
        if !L.with(|l| l.dragging.get()) {
            if (x - px).abs() < threshold && (y - py).abs() < threshold {
                return;
            }
            L.with(|l| l.dragging.set(true));
        }
        // Координаты внутри панели задач
        let vertical = tbr.h > tbr.w;
        let (nx, ny) = if vertical {
            (r.x - tbr.x, (y - gy - tbr.y).clamp(0, (tbr.h - r.h).max(0)))
        } else {
            ((x - gx - tbr.x).clamp(0, (tbr.w - r.w).max(0)), r.y - tbr.y)
        };
        SetWindowPos(hwnd, HWND_TOP, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        // Живой ряд: место меняется уже во время перетаскивания, соседи сразу расступаются
        let others: Vec<(i32, i32)> = neighbours(tb, hwnd)
            .into_iter()
            .map(|((o, _), r)| (o, if vertical { r.y + r.h / 2 } else { r.x + r.w / 2 }))
            .collect();
        let order = order_at(if vertical { y } else { x }, &others);
        if ORDER.swap(order, Ordering::SeqCst) != order {
            update_title();
            nudge_neighbours();
        }
    }

    /// Отпустили виджет в ряду: новый порядок по месту курсора, остальные подвинутся сами.
    unsafe fn drop_in_row(hwnd: HWND) {
        let (x, y) = cursor_pos();
        let tb = GetParent(hwnd);
        let vertical = win_rect(tb).is_some_and(|r| r.h > r.w);
        let others: Vec<(i32, i32)> = neighbours(tb, hwnd)
            .into_iter()
            .map(|((o, _), r)| (o, if vertical { r.y + r.h / 2 } else { r.x + r.w / 2 }))
            .collect();
        let order = order_at(if vertical { y } else { x }, &others);
        ORDER.store(order, Ordering::SeqCst);
        update_title();
        nudge_neighbours();
        emit(Event::Reorder(order)); // запомнить в настройках профиля
        L.with(|l| l.place.set((i32::MIN, 0, 0, 0))); // встать на место сейчас же
        ensure_widget();
    }

    // ---------- Рисование ----------

    unsafe fn fill(dc: *mut c_void, x: i32, y: i32, w: i32, hh: i32, c: COLORREF) {
        let r = RECT { left: x, top: y, right: x + w, bottom: y + hh };
        let b = CreateSolidBrush(c);
        FillRect(dc, &r, b);
        DeleteObject(b);
    }

    unsafe fn text_w(dc: *mut c_void, s: &[u16]) -> (i32, i32) {
        let mut sz = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, s.as_ptr(), s.len() as i32, &mut sz);
        (sz.cx, sz.cy)
    }

    /// WM_PAINT (сплошной фон и цветовой ключ): рисуем в память и копируем разом.
    unsafe fn paint(hwnd: HWND) {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(r) = win_rect(hwnd) {
            let (w, hh) = (r.w.max(1), r.h.max(1));
            let mem = CreateCompatibleDC(dc);
            let bmp = CreateCompatibleBitmap(dc, w, hh);
            let old_bmp = SelectObject(mem, bmp);
            draw(mem, r);
            BitBlt(dc, 0, 0, w, hh, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bmp);
            DeleteObject(bmp);
            DeleteDC(mem);
        }
        EndPaint(hwnd, &ps);
    }

    /// «Слоистый» виджет: рисуем в 32-битную картинку, фон (цвет-ключ) делаем почти
    /// прозрачным (альфа 1 — глазу не видно, но клик ловится), остальное — непрозрачным,
    /// и отдаём системе UpdateLayeredWindow. false — система не приняла (старый манифест и т. п.).
    unsafe fn push_alpha(hwnd: HWND) -> bool {
        let Some(r) = win_rect(hwnd) else { return false };
        let (w, hh) = (r.w.max(1), r.h.max(1));
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -hh; // строки сверху вниз
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB as _;
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if dib.is_null() || bits.is_null() {
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            return false;
        }
        let old = SelectObject(mem, dib);
        draw(mem, r);
        GdiFlush();
        // Пиксель DIB — 0xAARRGGBB, COLORREF — 0x00BBGGRR
        let k = key_color();
        let key_px = ((k & 0xff) << 16) | (k & 0xff00) | ((k >> 16) & 0xff);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * hh) as usize);
        for p in px.iter_mut() {
            *p = if *p & 0x00ff_ffff == key_px { 0x0100_0000 } else { *p | 0xff00_0000 };
        }
        if let Some(start) = L.with(|l| l.splat.get()) {
            let (sx, sy) = splat(start.elapsed().as_secs_f64() * 1000.0 / SPLAT_MS);
            squash(px, w, hh, sx, sy);
        }
        let size = SIZE { cx: w, cy: hh };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let ok = UpdateLayeredWindow(hwnd, screen, std::ptr::null(), &size, mem, &src, 0, &blend, ULW_ALPHA) != 0;
        SelectObject(mem, old);
        DeleteObject(dib);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        if !ok {
            applog::warn("панель задач: UpdateLayeredWindow не сработал — перехожу на цветовой ключ");
        }
        ok
    }

    /// Сжать/растянуть картинку относительно центра (кадр «шлепка»). Пустые места —
    /// почти прозрачные (альфа 1): клики по виджету ловятся и во время анимации.
    fn squash(px: &mut [u32], w: i32, hh: i32, sx: f64, sy: f64) {
        let src = px.to_vec();
        let (cx, cy) = (w as f64 / 2.0, hh as f64 / 2.0);
        for y in 0..hh {
            let syf = cy + (y as f64 + 0.5 - cy) / sy;
            for x in 0..w {
                let sxf = cx + (x as f64 + 0.5 - cx) / sx;
                let (ix, iy) = (sxf.floor() as i32, syf.floor() as i32);
                px[(y * w + x) as usize] = if ix >= 0 && iy >= 0 && ix < w && iy < hh {
                    src[(iy * w + ix) as usize]
                } else {
                    0x0100_0000
                };
            }
        }
    }

    /// Нарисовать виджет размера `r` в контекст `mem` (фон — цвет-ключ темы).
    unsafe fn draw(mem: *mut c_void, r: Rect) {
        let (w, hh) = (r.w.max(1), r.h.max(1));
        let d = data();
        let pal = palette();
        let dpi = L.with(|l| l.dpi.get());
        let scale = dpi as f64 / 96.0;
        let vertical = r.h > r.w && r.w < (60.0 * scale) as i32;
        let thick = if vertical { w } else { hh };
        let lay: Layout = layout(scale, thick, vertical, d.cat);

        fill(mem, 0, 0, w, hh, pal.key);

        // Котик
        if d.cat && lay.cat_cell > 0 {
            let f = L.with(|l| {
                let f = l.frame.get();
                if f.iter().flatten().all(|&c| c == 0) {
                    let nf = cat_frame(l.tick.get(), d.mood);
                    l.frame.set(nf);
                    nf
                } else {
                    f
                }
            });
            for (y, row) in f.iter().enumerate() {
                for (x, &c) in row.iter().enumerate() {
                    if let Some(col) = cat_color(c) {
                        let s = lay.cat_cell;
                        fill(mem, lay.cat_x + x as i32 * s, lay.cat_y + y as i32 * s, s, s, rgb3(col));
                    }
                }
            }
        }

        // Шрифт: Segoe UI, сглаживание без ClearType (цветная кайма на прозрачном фоне хуже)
        let mut lf: LOGFONTW = std::mem::zeroed();
        lf.lfHeight = -lay.font_px;
        lf.lfWeight = FW_NORMAL as _;
        lf.lfQuality = ANTIALIASED_QUALITY as _;
        for (i, ch) in "Segoe UI".encode_utf16().enumerate() {
            lf.lfFaceName[i] = ch;
        }
        let font = CreateFontIndirectW(&lf);
        let old_font = SelectObject(mem, font);
        SetBkMode(mem, TRANSPARENT as _);
        let text_c = if d.stale { pal.muted } else { pal.text };
        SetTextColor(mem, text_c);

        // «Шлепок» (B10): полоски заполняются волной
        let wave_t = L.with(|l| l.splat.get()).map(|s| s.elapsed().as_secs_f64() * 1000.0 / SPLAT_MS);
        let rows = [(crate::i18n::t("short.session"), d.session), (crate::i18n::t("short.weekly"), d.weekly)];
        for (i, (label, val)) in rows.iter().enumerate() {
            let wave = wave_t.map_or(1.0, |t| fill_wave(t, i));
            let cy = lay.row_y[i];
            let pct: Vec<u16> = pct_text(*val).encode_utf16().collect();
            let (pw, ph) = text_w(mem, &pct);
            if !lay.vertical {
                let lab: Vec<u16> = label.encode_utf16().collect();
                let (_, lh) = text_w(mem, &lab);
                TextOutW(mem, lay.label_x, cy - lh / 2, lab.as_ptr(), lab.len() as i32);
                let by = cy - lay.bar_h / 2;
                fill(mem, lay.bar_x, by, lay.bar_w, lay.bar_h, pal.track);
                if let Some(p) = val {
                    let fw = ((lay.bar_w as f64) * p.clamp(0.0, 100.0) / 100.0 * wave).round() as i32;
                    let c = if d.stale { pal.muted } else { rgb3(icons::level_color(*p)) };
                    fill(mem, lay.bar_x, by, fw, lay.bar_h, c);
                }
                TextOutW(mem, lay.pct_right - pw, cy - ph / 2, pct.as_ptr(), pct.len() as i32);
            } else {
                // Вертикально: процент по центру, под ним тонкая полоска
                TextOutW(mem, (w - pw) / 2, cy - ph / 2, pct.as_ptr(), pct.len() as i32);
                let by = cy + ph / 2;
                fill(mem, lay.bar_x, by, lay.bar_w, lay.bar_h, pal.track);
                if let Some(p) = val {
                    let fw = ((lay.bar_w as f64) * p.clamp(0.0, 100.0) / 100.0 * wave).round() as i32;
                    let c = if d.stale { pal.muted } else { rgb3(icons::level_color(*p)) };
                    fill(mem, lay.bar_x, by, fw, lay.bar_h, c);
                }
            }
        }

        SelectObject(mem, old_font);
        DeleteObject(font);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fits_taskbar() {
        // Windows 11, 100 %: панель 48 px
        let l = layout(1.0, 48, false, true);
        assert_eq!(l.h, 48);
        assert!(l.w > 100 && l.w < 160, "ширина {}", l.w);
        // Котик целиком по высоте и не налезает на подписи
        assert!(l.cat_y >= 0 && l.cat_y + 10 * l.cat_cell <= 48);
        assert!(l.cat_x + 12 * l.cat_cell < l.label_x);
        // Две строки внутри панели
        assert!(l.row_y[0] > 6 && l.row_y[1] < 42 && l.row_y[0] < l.row_y[1]);
        // 150 % — всё в полтора раза больше
        let big = layout(1.5, 72, false, true);
        assert!((big.w as f64 / l.w as f64 - 1.5).abs() < 0.1);
        // Без котика — уже
        assert!(layout(1.0, 48, false, false).w < l.w);
        // Вертикальная панель Windows 10 (ширина 62)
        let v = layout(1.0, 62, true, true);
        assert_eq!(v.w, 62);
        assert!(v.row_y[1] < v.h && v.bar_w > 0);
    }

    #[test]
    fn spot_left_of_tray() {
        // Панель снизу 1920×48, трей с x = 1700
        let tb = Rect { x: 0, y: 1032, w: 1920, h: 48 };
        let tray = Rect { x: 1700, y: 1032, w: 220, h: 48 };
        assert_eq!(spot(tb, Some(tray), 120, 48, 4), (1700 - 120 - 4, 0));
        // Второй монитор справа — координаты относительно панели
        let tb2 = Rect { x: 1920, y: 1032, w: 1920, h: 48 };
        let tray2 = Rect { x: 3600, y: 1032, w: 240, h: 48 };
        assert_eq!(spot(tb2, Some(tray2), 120, 48, 4), (3600 - 1920 - 124, 0));
        // Вертикальная панель слева, трей снизу
        let vtb = Rect { x: 0, y: 0, w: 62, h: 1080 };
        let vtray = Rect { x: 0, y: 900, w: 62, h: 180 };
        assert_eq!(spot(vtb, Some(vtray), 62, 90, 4), (0, 900 - 94));
        // Трей не нашли — правая четверть
        assert_eq!(spot(tb, None, 120, 48, 4), (1440 - 120, 0));
        // Трей пустой (0 × 0) — тоже «не нашли»
        assert_eq!(spot(tb, Some(Rect { x: 0, y: 0, w: 0, h: 0 }), 120, 48, 4), (1320, 0));
    }

    #[test]
    fn two_profiles_in_a_row() {
        // Основной (порядок 0) — у трея, профиль 2 (порядок 100) — левее
        assert_eq!(queue_offset((0, 0), &[((100, 1), 128)], 4), 0);
        assert_eq!(queue_offset((100, 1), &[((0, 0), 128)], 4), 132);
        // Третий — за двумя; кто дальше от трея, не мешает
        assert_eq!(queue_offset((200, 2), &[((0, 0), 128), ((100, 1), 100), ((500, 5), 90)], 4), 132 + 104);
        // Одинаковый порядок — решает номер профиля
        assert_eq!(queue_offset((0, 1), &[((0, 0), 128)], 4), 132);
        assert_eq!(parse_widget_title(&widget_title((-150, 3))), Some((-150, 3)));
        assert_eq!(parse_widget_title("TokenCat:7"), None);
        assert_eq!(default_order(2), 200);
    }

    #[test]
    fn drop_between_neighbours() {
        // Трей справа. Соседи: порядок 0 (центр 1640), 100 (центр 1500)
        let others = [(0, 1640), (100, 1500)];
        // Бросили правее всех (у трея) — ближе всех к трею
        assert_eq!(order_at(1700, &others), -100);
        // Между ними
        assert_eq!(order_at(1570, &others), 50);
        // Левее всех
        assert_eq!(order_at(1200, &others), 200);
        // Соседей нет
        assert_eq!(order_at(500, &[]), 0);
        // Соседи вплотную по порядку
        assert_eq!(order_at(1570, &[(0, 1640), (1, 1500)]), 1);
    }

    #[test]
    fn splat_curve() {
        let (sx, sy) = splat(0.0);
        assert!(sx < 1.0 && sy > 1.0, "начало — узкий и вытянутый");
        let (sx, sy) = splat(0.4);
        assert!(sx > 1.0 && sy < 1.0, "середина — сплющен");
        assert_eq!(splat(1.0), (1.0, 1.0));
        assert_eq!(splat(5.0), (1.0, 1.0));
        assert_eq!(fill_wave(0.0, 0), 0.0);
        assert!(fill_wave(0.5, 0) > fill_wave(0.5, 1), "вторая строка заполняется позже");
        assert_eq!(fill_wave(1.0, 1), 1.0);
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5);
    }

    #[test]
    fn percent_text() {
        assert_eq!(pct_text(Some(41.6)), "42%");
        assert_eq!(pct_text(Some(100.0)), "100%");
        assert_eq!(pct_text(None), "—");
    }
}
