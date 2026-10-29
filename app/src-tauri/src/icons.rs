// Картинки для значков (B2): котик, цифры в трее, кольцо — без библиотек, просто RGBA.
//
// - Котик — тот же спрайт и те же кадры, что в ui/cat.js (там подробные комментарии).
//   Его рисует виджет панели задач Windows и значок строки меню macOS.
// - «Цифры в трее» — запасной вид Windows (если встроиться в панель задач не удалось):
//   процент 5-часового окна крупно + полоска недели снизу.
// - «Кольцо» — вариант строки меню macOS.
// Пороги цвета те же, что у полосок панели (tokens.css): 0–49 зелёный, 50–79 жёлтый, 80+ красный.

/// RGBA-картинка: ширина, высота, пиксели построчно.
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Rgba {
    pub fn new(w: u32, h: u32) -> Self {
        Rgba { w, h, px: vec![0; (w * h * 4) as usize] }
    }

    pub fn put(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        self.px[i..i + 4].copy_from_slice(&c);
    }

    #[allow(dead_code)] // нужен тестам
    pub fn get(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return [0; 4];
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, c);
            }
        }
    }
}

// ---------- Цвета ----------

pub const GREEN: [u8; 3] = [0x4c, 0xc3, 0x8a];
pub const YELLOW: [u8; 3] = [0xf5, 0xc5, 0x42];
pub const RED: [u8; 3] = [0xe5, 0x48, 0x4d];

/// Цвет по порогу, как у полосок панели.
pub fn level_color(p: f64) -> [u8; 3] {
    if p >= 80.0 {
        RED
    } else if p >= 50.0 {
        YELLOW
    } else {
        GREEN
    }
}

fn rgba(c: [u8; 3], a: u8) -> [u8; 4] {
    [c[0], c[1], c[2], a]
}

// ---------- Котик ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Idle,
    Sleep,
    Alert,
}

/// Сетка спрайта с контуром: 12 × 10 клеток (x −1..10, y −1..8 со сдвигом на 1).
pub const CAT_W: usize = 12;
pub const CAT_H: usize = 10;
/// Клетка: 0 — пусто, иначе буква: w — белый, g — тень, k — глаза, p — нос, o — контур.
pub type CatFrame = [[u8; CAT_W]; CAT_H];

const HEAD: [&str; 5] = ["w.....w", "wwwwwww", "wkwwwkw", "wwwpwww", ".wwwww."];
const HEAD_BLINK: [&str; 5] = ["w.....w", "wwwwwww", "wgwwwgw", "wwwpwww", ".wwwww."];
const HEAD_EAR: [&str; 5] = [".w....w", "wwwwwww", "wkwwwkw", "wwwpwww", ".wwwww."];
const BODY: [&str; 3] = [".wwwww.", "wwwwwww", "wgwwwgw"];
const TAILS: [[(i32, i32); 3]; 4] = [
    [(7, 2), (8, 1), (8, 0)],
    [(7, 2), (8, 2), (8, 1)],
    [(7, 2), (8, 1), (9, 0)],
    [(7, 2), (8, 1), (8, 0)],
];
const TAIL_DOWN: [(i32, i32); 3] = [(7, 2), (8, 2), (9, 2)];

/// Кадр котика для тика t (8 тиков в секунду) — как frame() в cat.js.
pub fn cat_frame(t: u64, mood: Mood) -> CatFrame {
    let (breathe, head, tail): (i32, &[&str; 5], &[(i32, i32); 3]) = match mood {
        Mood::Sleep => ((t / 16 % 2) as i32, &HEAD_BLINK, &TAIL_DOWN),
        Mood::Alert => ((t / 4 % 2) as i32, &HEAD, &TAILS[(t % 4) as usize]),
        Mood::Idle => {
            let head = if t % 46 <= 1 {
                &HEAD_BLINK
            } else if [30, 31, 33].contains(&(t % 97)) {
                &HEAD_EAR
            } else {
                &HEAD
            };
            ((t / 10 % 2) as i32, head, &TAILS[(t / 3 % 4) as usize])
        }
    };
    // Сначала тело, хвост, голова (без контура) в координатах −1..10
    let mut body = [[0u8; CAT_W]; CAT_H];
    let mut set = |x: i32, y: i32, c: u8| {
        let (gx, gy) = (x + 1, y + 1);
        if gx >= 0 && gy >= 0 && (gx as usize) < CAT_W && (gy as usize) < CAT_H {
            body[gy as usize][gx as usize] = c;
        }
    };
    for (y, row) in BODY.iter().enumerate() {
        for (x, c) in row.bytes().enumerate() {
            if c != b'.' {
                set(x as i32, y as i32 + 5, c);
            }
        }
    }
    for &(x, y) in tail {
        set(x, y + 5, b'w');
    }
    for (y, row) in head.iter().enumerate() {
        for (x, c) in row.bytes().enumerate() {
            if c != b'.' {
                set(x as i32, y as i32 + breathe, c);
            }
        }
    }
    // Контур: пустые соседи закрашенных клеток
    let mut out = body;
    for y in 0..CAT_H as i32 {
        for x in 0..CAT_W as i32 {
            if body[y as usize][x as usize] == 0 {
                continue;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < CAT_W && (ny as usize) < CAT_H
                    && body[ny as usize][nx as usize] == 0
                {
                    out[ny as usize][nx as usize] = b'o';
                }
            }
        }
    }
    out
}

/// Цвет клетки котика (RGB), как COLORS в cat.js.
pub fn cat_color(c: u8) -> Option<[u8; 3]> {
    match c {
        b'w' => Some([0xff, 0xff, 0xff]),
        b'g' => Some([0xd9, 0xd6, 0xcf]),
        b'k' => Some([0x2b, 0x2a, 0x28]),
        b'p' => Some([0xf2, 0xa7, 0xb0]),
        b'o' => Some([0x14, 0x13, 0x12]),
        _ => None,
    }
}

/// Силуэт котика для строки меню macOS (квадрат side × side, по центру).
/// `color` = None — шаблонное изображение (чёрное, macOS сама перекрасит под светлую/тёмную
/// строку); Some — цветной силуэт (порог 50 %+). Глаза — прорези.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn cat_silhouette(side: u32, color: Option<[u8; 3]>) -> Rgba {
    // Тик 5: глаза открыты (на тике 0 котик моргает)
    let f = cat_frame(5, Mood::Idle);
    let mut img = Rgba::new(side, side);
    // Без контура: клетки x 1..10, y 1..8 → 10 × 9 (берём ширину 11 с хвостом)
    let cell = (side as usize / CAT_W).max(1) as i32;
    let ox = (side as i32 - CAT_W as i32 * cell) / 2;
    let oy = (side as i32 - CAT_H as i32 * cell) / 2;
    let c = color.unwrap_or([0, 0, 0]);
    for (y, row) in f.iter().enumerate() {
        for (x, &v) in row.iter().enumerate() {
            if matches!(v, b'w' | b'g' | b'p') {
                img.rect(ox + x as i32 * cell, oy + y as i32 * cell, cell, cell, rgba(c, 255));
            }
        }
    }
    img
}

// ---------- Цифры ----------

/// Цифры 3 × 5: каждая строка — 3 бита (старший — левый пиксель).
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// Значок трея «цифры» 32 × 32 (Windows покажет его 16 × 16 при 100 % — клетки по 2 px).
/// `session` — процент 5-часового окна (None — нет данных: прочерк), `weekly` — полоска снизу.
/// `stale` — данные устарели: цифры бледнее.
pub fn tray_numbers(session: Option<f64>, weekly: Option<f64>, stale: bool) -> Rgba {
    const S: u32 = 32;
    let mut img = Rgba::new(S, S);
    let alpha = if stale { 150 } else { 255 };
    let outline = [0x14, 0x13, 0x12, alpha];

    // Цифры: 1–2 знака клеткой 4 px, «100» — клеткой 3 px
    let (text, cell, gap) = match session {
        Some(p) => {
            let v = p.round().clamp(0.0, 100.0) as u32;
            if v >= 100 {
                ("100".to_string(), 3, 2)
            } else {
                (v.to_string(), 4, 4)
            }
        }
        None => (String::new(), 4, 4),
    };
    let color = rgba(session.map(level_color).unwrap_or([0x8f, 0x8c, 0x84]), alpha);
    let n = text.len() as i32;
    let tw = n * 3 * cell + (n - 1).max(0) * gap;
    let x0 = (S as i32 - tw) / 2;
    let y0 = 3;
    // Сначала заливка цифр в отдельную маску, потом контур вокруг (виден на светлой и тёмной панели)
    let mut mask = vec![false; (S * S) as usize];
    let mut mark = |x: i32, y: i32| {
        if x >= 0 && y >= 0 && x < S as i32 && y < S as i32 {
            mask[(y as u32 * S + x as u32) as usize] = true;
        }
    };
    if text.is_empty() {
        // Прочерк — нет данных
        for y in 13..17 {
            for x in 8..24 {
                mark(x, y);
            }
        }
    }
    for (i, ch) in text.bytes().enumerate() {
        let d = DIGITS[(ch - b'0') as usize];
        let dx = x0 + i as i32 * (3 * cell + gap);
        for (ry, bits) in d.iter().enumerate() {
            for rx in 0..3 {
                if bits & (0b100 >> rx) != 0 {
                    for yy in 0..cell {
                        for xx in 0..cell {
                            mark(dx + rx * cell + xx, y0 + ry as i32 * cell + yy);
                        }
                    }
                }
            }
        }
    }
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            if mask[(y as u32 * S + x as u32) as usize] {
                continue;
            }
            let near = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < S as i32 && ny < S as i32 && mask[(ny as u32 * S + nx as u32) as usize]
            });
            if near {
                img.put(x, y, outline);
            }
        }
    }
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            if mask[(y as u32 * S + x as u32) as usize] {
                img.put(x, y, color);
            }
        }
    }

    // Полоска недели: y 25..31, с тёмной рамкой
    if let Some(w) = weekly {
        let (bx, by, bw, bh) = (2, 25, 28, 6);
        img.rect(bx, by, bw, bh, outline);
        img.rect(bx + 1, by + 1, bw - 2, bh - 2, [0x3a, 0x39, 0x36, alpha]);
        let fill = ((bw - 2) as f64 * w.clamp(0.0, 100.0) / 100.0).round() as i32;
        img.rect(bx + 1, by + 1, fill, bh - 2, rgba(level_color(w), alpha));
    }
    img
}

// ---------- Кольцо ----------

/// Кольцо-прогресс для строки меню macOS: дорожка + дуга от 12 часов по часовой стрелке.
/// Сглаживание — 4 × 4 подвыборки на пиксель.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn ring(side: u32, percent: Option<f64>) -> Rgba {
    let mut img = Rgba::new(side, side);
    let c = side as f64 / 2.0;
    let r_out = c - 1.0;
    let r_in = r_out - (side as f64 * 0.17).max(2.0);
    let p = percent.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
    let color = percent.map(level_color).unwrap_or([0x8f, 0x8c, 0x84]);
    let track = [0x8f, 0x8c, 0x84];
    for y in 0..side {
        for x in 0..side {
            let (mut fill, mut tr) = (0u32, 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let fx = x as f64 + (sx as f64 + 0.5) / 4.0 - c;
                    let fy = y as f64 + (sy as f64 + 0.5) / 4.0 - c;
                    let d = (fx * fx + fy * fy).sqrt();
                    if d < r_in || d > r_out {
                        continue;
                    }
                    // Угол от 12 часов по часовой стрелке, 0..1
                    let a = (fx.atan2(-fy) / std::f64::consts::TAU).rem_euclid(1.0);
                    if a < p {
                        fill += 1;
                    } else {
                        tr += 1;
                    }
                }
            }
            if fill + tr == 0 {
                continue;
            }
            // Смешиваем дорожку (полупрозрачную) и дугу
            let fa = fill as f64 / 16.0;
            let ta = tr as f64 / 16.0 * 0.45;
            let a = fa + ta;
            let mix = |i: usize| ((color[i] as f64 * fa + track[i] as f64 * ta) / a).round() as u8;
            img.put(x as i32, y as i32, [mix(0), mix(1), mix(2), (a * 255.0).round().min(255.0) as u8]);
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(f: &CatFrame, c: u8) -> usize {
        f.iter().flatten().filter(|&&v| v == c).count()
    }

    #[test]
    fn cat_like_js() {
        let f = cat_frame(5, Mood::Idle);
        assert_eq!(count(&f, b'k'), 2, "два глаза");
        assert_eq!(count(&f, b'p'), 1, "нос");
        assert!(count(&f, b'o') > 20, "есть контур");
        // Спит — глаза закрыты
        assert_eq!(count(&cat_frame(0, Mood::Sleep), b'k'), 0);
        // Моргает в начале каждых 46 тиков
        assert_eq!(count(&cat_frame(46, Mood::Idle), b'k'), 0);
        // Котик целиком в сетке: по краям — только контур или пусто
        for row in f.iter() {
            assert!(matches!(row[0], 0 | b'o') && matches!(row[CAT_W - 1], 0 | b'o'));
        }
    }

    #[test]
    fn levels() {
        assert_eq!(level_color(0.0), GREEN);
        assert_eq!(level_color(49.9), GREEN);
        assert_eq!(level_color(50.0), YELLOW);
        assert_eq!(level_color(80.0), RED);
    }

    #[test]
    fn numbers_icon() {
        let i = tray_numbers(Some(42.0), Some(18.0), false);
        assert_eq!((i.w, i.h, i.px.len()), (32, 32, 32 * 32 * 4));
        // Левая палочка «4» (клетка 4 px, начало x = (32 − 28) / 2 = 2) — зелёная
        assert_eq!(i.get(3, 4), [GREEN[0], GREEN[1], GREEN[2], 255]);
        // Полоска недели: начало залито, конец — дорожка
        assert_eq!(&i.get(4, 27)[..3], &GREEN);
        assert_eq!(&i.get(28, 27)[..3], &[0x3a, 0x39, 0x36]);
        // «100» помещается и красное
        let full = tray_numbers(Some(100.0), None, false);
        assert!(full.px.chunks(4).any(|p| p[..3] == RED));
        // Нет данных — прочерк, без паники
        let none = tray_numbers(None, None, true);
        assert!(none.px.chunks(4).any(|p| p[3] > 0));
    }

    #[test]
    fn ring_and_silhouette() {
        let r = ring(36, Some(25.0));
        // Верх-право (четверть пройдена) — цвет дуги, низ — только дорожка
        let top_right = r.get(30, 6);
        assert!(top_right[3] > 0 && top_right[1] > top_right[0], "зелёная дуга справа сверху");
        let bottom = r.get(18, 34);
        assert!(bottom[3] > 0 && bottom[3] < 200, "полупрозрачная дорожка снизу");
        assert_eq!(r.get(18, 18)[3], 0, "в центре пусто");

        let t = cat_silhouette(36, None);
        assert!(t.px.chunks(4).any(|p| p == [0, 0, 0, 255]));
        assert!(t.px.chunks(4).all(|p| p[3] == 0 || p[..3] == [0, 0, 0]));
        // Глаза — прорези: в силуэте есть прозрачные клетки внутри головы
        let cell = 36 / CAT_W as i32;
        let (ox, oy) = ((36 - CAT_W as i32 * cell) / 2, (36 - CAT_H as i32 * cell) / 2);
        assert_eq!(t.get(ox + 2 * cell, oy + 3 * cell)[3], 0, "левый глаз");
    }
}
