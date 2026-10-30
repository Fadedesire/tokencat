// Ранний старт 5-часового окна и котофакт дня (B9). Только расчёты, без Tauri и DOM —
// поэтому тот же файл проверяют тесты (tools/test-ui.js, запускает test.bat через Node).
//
// Как работает ранний старт (решение B9 п. 1, DECISIONS): запрос шлёт не TokenCat, а
// запланированная задача Claude в облаке. Пользователь выбирает час, к которому хочет сброс
// (например 11:00), задача стоит на 5 часов раньше плюс 1 минута (6:01): claude.ai
// округляет начало окна вниз до часа (по справке) или считает от минуты — +1 мин даёт
// нужный час при любом правиле. TokenCat по `resets_at` 5-часового окна показывает,
// сработала ли задача сегодня.
(function (root) {
  "use strict";

  const HOUR = 3600e3, MIN = 60e3;
  const WINDOW_H = 5;          // длина окна подписки, часы
  const TASK_MIN = 1;          // минута запуска задачи после часа

  const pad = (n) => String(n).padStart(2, "0");
  const hhmm = (h, m) => pad(h) + ":" + pad(m);

  /** Во сколько ставить задачу для сброса в resetHour:00 → { h, m, prevDay }. */
  function taskTime(resetHour) {
    const h = (((resetHour - WINDOW_H) % 24) + 24) % 24;
    return { h, m: TASK_MIN, prevDay: resetHour < WINDOW_H };
  }

  /** «06:01» для сброса в 11. */
  function taskLabel(resetHour) {
    const t = taskTime(resetHour);
    return hhmm(t.h, t.m);
  }

  /** Местная полночь дня, в который попадает момент t (мс). */
  function dayStart(t) {
    const d = new Date(t);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  }

  /** Момент «день dayMs (полночь) + h:m» по местным часам — с учётом перехода на летнее время. */
  function at(dayMs, h, m) {
    const d = new Date(dayMs);
    d.setHours(h, m, 0, 0);
    return d.getTime();
  }

  /** Ключ дня «2026-10-30» по местному времени. */
  function dayKey(t = Date.now()) {
    const d = new Date(t);
    return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate());
  }

  /**
   * Цикл раннего старта, в котором сейчас находимся: от запуска задачи до часа после
   * желаемого сброса. Возвращает { task, reset } (мс) или null, если сейчас между циклами.
   */
  function currentCycle(resetHour, now) {
    const today = dayStart(now);
    for (const shift of [-1, 0, 1]) {
      // Полночь соседнего дня: +-36 ч от полудня, чтобы не споткнуться о 23/25-часовые сутки
      const day = dayStart(today + 12 * HOUR + shift * 24 * HOUR);
      const reset = at(day, resetHour, 0);
      const t = taskTime(resetHour);
      const task = at(t.prevDay ? dayStart(day - 12 * HOUR) : day, t.h, t.m);
      if (now >= task && now < reset + HOUR) return { task, reset };
    }
    return null;
  }

  /** Ближайший запуск задачи после now (мс). */
  function nextTask(resetHour, now) {
    const t = taskTime(resetHour);
    let day = dayStart(now);
    for (let i = 0; i < 3; i++) {
      const ms = at(day, t.h, t.m);
      if (ms > now) return ms;
      day = dayStart(day + 36 * HOUR);
    }
    return now + 24 * HOUR;
  }

  function clock(ms) {
    const d = new Date(ms);
    return hhmm(d.getHours(), d.getMinutes());
  }

  /**
   * Что сказать о раннем старте сейчас.
   *   session — 5-часовой лимит из снимка ({ percent, resets_at }) или null (нет данных).
   * Ответ: { kind: "off" | "wait" | "ok" | "miss" | "other" | "unknown", text }.
   */
  function status(opts) {
    const { enabled, resetHour, session, now = Date.now(), haveData = true } = opts;
    if (!enabled) return { kind: "off", text: "" };
    const want = hhmm(resetHour, 0);
    const cyc = currentCycle(resetHour, now);
    if (!cyc) {
      return { kind: "wait", text: `Задача запустит окно в ${clock(nextTask(resetHour, now))} — сброс будет в ${want}.` };
    }
    if (!haveData || !session) {
      return { kind: "unknown", text: `Сброс ждём в ${want}. Проверю, когда появятся данные о лимитах.` };
    }
    const r = session.resets_at ? Date.parse(session.resets_at) : NaN;
    if (!isNaN(r) && r >= cyc.reset && r < cyc.reset + HOUR) {
      return { kind: "ok", text: `Сработало: 5-часовой лимит сбросится в ${clock(r)}.` };
    }
    if (isNaN(r)) {
      // Окно не начато, хотя задача уже должна была его запустить (дадим ей 15 минут)
      if (now - cyc.task < 15 * MIN) return { kind: "wait", text: `Задача запускается (${clock(cyc.task)})…` };
      return { kind: "miss", text: `Окно не началось: похоже, задача в Claude в ${clock(cyc.task)} не сработала. Проверьте её в разделе «Запланированные».` };
    }
    if (r <= now) return { kind: "unknown", text: `Сброс ждём в ${want}.` };
    return { kind: "other", text: `Сейчас сброс в ${clock(r)}, а не в ${want}: окно началось раньше задачи (или задача не сработала).` };
  }

  /** Номер дня от 1970 по местному календарю — для выбора факта. */
  function dayNumber(t = Date.now()) {
    const d = new Date(t);
    return Math.floor(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / (24 * HOUR));
  }

  /** Котофакт дня: один и тот же весь день, на следующий день — следующий по кругу. */
  function factOfDay(facts, t = Date.now()) {
    if (!facts || !facts.length) return null;
    return facts[dayNumber(t) % facts.length];
  }

  /** Показывать ли облачко: включено, котик на месте, сегодня ещё не закрывали. */
  function showFact(opts) {
    const { enabled, cat, hiddenDay, now = Date.now() } = opts;
    return !!enabled && !!cat && hiddenDay !== dayKey(now);
  }

  /** Текст задачи для Claude: короткий запрос, крошечный ответ. */
  const PROMPT = "Расскажи один короткий интересный факт про котов — одним предложением.";

  const api = { taskTime, taskLabel, currentCycle, nextTask, status, dayKey, dayNumber, factOfDay, showFact, PROMPT, hhmm };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else (root.TokenCat = root.TokenCat || {}).early = api;
})(typeof window !== "undefined" ? window : globalThis);
