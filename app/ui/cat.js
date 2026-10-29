// TokenCat — пиксельный котик (S06). Спрайт и логика кадров перенесены из прототипа
// (prototype/claude_limits.pyw: CAT_HEAD, CAT_BODY, CAT_TAILS, cat_frame).
//
// Нагрузка: один setTimeout раз в 125 мс (8 кадров/с, как в прототипе), рисуем только
// если кадр изменился. Когда окно скрыто (document.hidden), таймер не крутится вообще.
// Чёткость: размер «пикселя» котика переводится в целые физические пиксели экрана с учётом
// devicePixelRatio (100/125/150 %), canvas рисуется 1:1 без сглаживания.
(function () {
  "use strict";

  // w — белый, g — тень, k — глаза, p — нос, o — контур (считается сам)
  const HEAD = ["w.....w", "wwwwwww", "wkwwwkw", "wwwpwww", ".wwwww."];
  const HEAD_BLINK = [HEAD[0], HEAD[1], "wgwwwgw", HEAD[3], HEAD[4]];
  const HEAD_EAR = [".w....w", ...HEAD.slice(1)];
  const BODY = [".wwwww.", "wwwwwww", "wgwwwgw"];
  const TAILS = [
    [[7, 2], [8, 1], [8, 0]],
    [[7, 2], [8, 2], [8, 1]],
    [[7, 2], [8, 1], [9, 0]],
    [[7, 2], [8, 1], [8, 0]],
  ];
  const TAIL_DOWN = [[7, 2], [8, 2], [9, 2]];   // спит: хвост лежит
  const COLORS = { w: "#ffffff", g: "#d9d6cf", k: "#2b2a28", p: "#f2a7b0", o: "#141312" };

  // Сетка спрайта с контуром: x −1..10, y −1..8 → 12 × 10 клеток
  const OX = 1, OY = 1, GW = 12, GH = 10;

  /**
   * Пиксели котика для тика t (≈ 8 тиков в секунду).
   * mood: "idle" — обычный; "sleep" — нет данных/не вошли; "alert" — лимит 80 %+.
   * Возвращает Map "x,y" → цвет-буква (с контуром).
   */
  function frame(t, mood) {
    let breathe, head, tail;
    if (mood === "sleep") {
      breathe = (Math.floor(t / 16) % 2) ? 1 : 0;    // дышит медленнее
      head = HEAD_BLINK;                             // глаза закрыты
      tail = TAIL_DOWN;
    } else if (mood === "alert") {
      breathe = (Math.floor(t / 4) % 2) ? 1 : 0;     // дышит часто
      head = HEAD;                                   // не моргает, уши торчком
      tail = TAILS[t % TAILS.length];                // хвост мечется
    } else {
      breathe = (Math.floor(t / 10) % 2) ? 1 : 0;
      head = HEAD;
      if (t % 46 === 0 || t % 46 === 1) head = HEAD_BLINK;
      else if ([30, 31, 33].includes(t % 97)) head = HEAD_EAR;
      tail = TAILS[Math.floor(t / 3) % TAILS.length];
    }
    const px = new Map();
    BODY.forEach((row, y) => [...row].forEach((c, x) => { if (c !== ".") px.set(x + "," + (y + 5), c); }));
    for (const [x, y] of tail) px.set(x + "," + (y + 5), "w");
    head.forEach((row, y) => [...row].forEach((c, x) => { if (c !== ".") px.set(x + "," + (y + breathe), c); }));
    const out = new Map();
    for (const key of px.keys()) {
      const [x, y] = key.split(",").map(Number);
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const q = (x + dx) + "," + (y + dy);
        if (!px.has(q)) out.set(q, "o");
      }
    }
    for (const [k, c] of px) out.set(k, c);
    return out;
  }

  const reduceMotion = () =>
    !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);

  const all = new Set();        // живые котики — общий таймер на всех
  let timer = null;
  let tick = 0;

  function loop() {
    timer = null;
    if (document.hidden || all.size === 0) return;   // окно скрыто — спим, без таймера
    tick++;
    for (const cat of all) cat._draw(tick);
    timer = setTimeout(loop, 125);
  }
  function ensureLoop() {
    if (!timer && !document.hidden && all.size && !reduceMotion()) timer = setTimeout(loop, 125);
  }
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) { clearTimeout(timer); timer = null; }
    else { for (const c of all) c._draw(tick, true); ensureLoop(); }
  });

  class Cat {
    /**
     * canvas — элемент <canvas>; opts.px — размер «пикселя» котика в CSS-пикселях (по умолчанию 2);
     * opts.mood — "idle" | "sleep" | "alert"; opts.animate=false — статичный кадр.
     */
    constructor(canvas, opts = {}) {
      this.canvas = canvas;
      this.ctx = canvas.getContext("2d");
      this.px = opts.px || 2;
      this.mood = opts.mood || "idle";
      this.animate = opts.animate !== false;
      this.last = "";
      this._fit();
      // Смена масштаба экрана (перетащили на другой монитор) — пересчитать размер
      this._watchDpr();
      this._draw(tick, true);
      if (this.animate) { all.add(this); ensureLoop(); }
    }

    setMood(mood) {
      if (mood === this.mood) return;
      this.mood = mood;
      this._draw(tick, true);
    }

    destroy() { all.delete(this); }

    _watchDpr() {
      if (!window.matchMedia) return;
      const mq = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      const onChange = () => { this._fit(); this._draw(tick, true); this._watchDpr(); };
      mq.addEventListener ? mq.addEventListener("change", onChange, { once: true })
                          : mq.addListener(onChange);
    }

    _fit() {
      const dpr = window.devicePixelRatio || 1;
      // Целое число физических пикселей на клетку спрайта: без полутонов и размытия
      this.scale = Math.max(1, Math.round(this.px * dpr));
      this.canvas.width = GW * this.scale;
      this.canvas.height = GH * this.scale;
      const cssCell = this.scale / dpr;
      this.canvas.style.width = (GW * cssCell) + "px";
      this.canvas.style.height = (GH * cssCell) + "px";
      // Лапы стоят на линии: ниже неё остаётся только ряд контура (используется в шапке панели)
      this.canvas.style.setProperty("--tc-cat-cell", cssCell + "px");
      this.cssCell = cssCell;
      this.last = "";
    }

    _draw(t, force) {
      const f = frame(this.animate && !reduceMotion() ? t : 0, this.mood);
      const key = [...f].map(([k, c]) => k + c).join("");
      if (!force && key === this.last) return;   // кадр не изменился — не рисуем
      this.last = key;
      const { ctx, scale: s } = this;
      ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
      for (const [k, c] of f) {
        const [x, y] = k.split(",").map(Number);
        ctx.fillStyle = COLORS[c];
        ctx.fillRect((x + OX) * s, (y + OY) * s, s, s);
      }
    }
  }

  Cat.frame = frame;
  Cat.GRID = { w: GW, h: GH };
  window.TokenCat = window.TokenCat || {};
  window.TokenCat.Cat = Cat;
})();
