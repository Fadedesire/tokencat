// Тесты расчётов интерфейса (B9: ранний старт и котофакт). Запуск: node tools\test-ui.js
// (это делает test.bat после cargo test). Каждый набор гоняется в нескольких часовых поясах:
// процесс перезапускает сам себя с другим TZ — Node читает пояс при старте.
"use strict";
const path = require("path");
const { execFileSync } = require("child_process");

const ZONES = ["Europe/Moscow", "America/New_York", "Europe/Berlin", "Asia/Tokyo", "Australia/Lord_Howe"];

if (!process.env.TC_TEST_CHILD) {
  let failed = 0;
  for (const tz of ZONES) {
    try {
      const out = execFileSync(process.execPath, [__filename], {
        env: { ...process.env, TZ: tz, TC_TEST_CHILD: "1" }, encoding: "utf8",
      });
      process.stdout.write(out);
    } catch (e) {
      failed++;
      process.stdout.write((e.stdout || "") + (e.stderr || ""));
    }
  }
  console.log(failed ? `\nUI-тесты: упало в ${failed} поясах` : "\nUI-тесты: все прошли");
  process.exit(failed ? 1 : 0);
}

// ---------- Дочерний процесс: тесты в одном поясе ----------
const E = require(path.join(__dirname, "..", "app", "ui", "early.js"));
const FACTS = require(path.join(__dirname, "..", "app", "ui", "catfacts.js"));
const tz = process.env.TZ;
let n = 0, bad = 0;
function ok(cond, name) {
  n++;
  if (!cond) { bad++; console.log(`  [${tz}] ПРОВАЛ: ${name}`); }
}
const eq = (a, b, name) => ok(JSON.stringify(a) === JSON.stringify(b), `${name}: ${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
/** Местное время: год, месяц (1–12), день, ч, м. */
const L = (y, mo, d, h, m = 0) => new Date(y, mo - 1, d, h, m).getTime();
const iso = (ms) => new Date(ms).toISOString();

// 1. Время задачи: сброс − 5 ч + 1 мин, через полночь — накануне
eq(E.taskTime(11), { h: 6, m: 1, prevDay: false }, "сброс 11 → 6:01");
eq(E.taskTime(5), { h: 0, m: 1, prevDay: false }, "сброс 5 → 0:01");
eq(E.taskTime(3), { h: 22, m: 1, prevDay: true }, "сброс 3 → 22:01 накануне");
eq(E.taskTime(0), { h: 19, m: 1, prevDay: true }, "сброс 0 → 19:01 накануне");
eq(E.taskLabel(14), "09:01", "подпись 14 → 09:01");

// 2. Цикл и следующий запуск
{
  const now = L(2026, 10, 30, 8, 0);
  const c = E.currentCycle(11, now);
  ok(c && c.task === L(2026, 10, 30, 6, 1) && c.reset === L(2026, 10, 30, 11, 0), "цикл 30.10 6:01–11:00");
  ok(E.currentCycle(11, L(2026, 10, 30, 5, 0)) === null, "в 5:00 — между циклами");
  ok(E.currentCycle(11, L(2026, 10, 30, 12, 30)) === null, "в 12:30 — цикл закончился");
  eq(E.nextTask(11, L(2026, 10, 30, 12, 30)), L(2026, 10, 31, 6, 1), "следующая задача завтра в 6:01");
  eq(E.nextTask(11, L(2026, 10, 30, 5, 0)), L(2026, 10, 30, 6, 1), "следующая задача сегодня в 6:01");
  // Через полночь: сброс в 3:00, задача накануне в 22:01
  const c2 = E.currentCycle(3, L(2026, 10, 30, 23, 30));
  ok(c2 && c2.task === L(2026, 10, 30, 22, 1) && c2.reset === L(2026, 10, 31, 3, 0), "через полночь: 22:01 → 3:00");
  const c3 = E.currentCycle(3, L(2026, 10, 31, 1, 0));
  ok(c3 && c3.reset === L(2026, 10, 31, 3, 0), "после полуночи тот же цикл");
}

// 3. Статус по resets_at
{
  const now = L(2026, 10, 30, 9, 0);
  const st = (session, extra = {}) => E.status({ enabled: true, resetHour: 11, session, now, ...extra }).kind;
  eq(E.status({ enabled: false, resetHour: 11, session: null, now }).kind, "off", "выключено");
  eq(st({ percent: 3, resets_at: iso(L(2026, 10, 30, 11, 0)) }), "ok", "сброс ровно в 11:00");
  eq(st({ percent: 3, resets_at: iso(L(2026, 10, 30, 11, 1) + 638) }), "ok", "сброс в 11:01 (от минуты)");
  eq(st({ percent: 0, resets_at: null }), "miss", "окно не начато в 9:00 — задача не сработала");
  eq(E.status({ enabled: true, resetHour: 11, session: { percent: 0, resets_at: null }, now: L(2026, 10, 30, 6, 10) }).kind,
    "wait", "6:10 — даём задаче 15 минут");
  eq(st({ percent: 40, resets_at: iso(L(2026, 10, 30, 13, 20)) }), "other", "окно началось в 8:20 — не от задачи");
  eq(st(null, { haveData: false }), "unknown", "нет данных");
  eq(E.status({ enabled: true, resetHour: 11, session: null, now: L(2026, 10, 30, 4, 0) }).kind, "wait", "до задачи — ждём");
  ok(/06:01/.test(E.status({ enabled: true, resetHour: 11, session: null, now: L(2026, 10, 30, 4, 0) }).text), "текст с 06:01");
}

// 4. Летнее время: сутки 23 и 25 часов (в поясах без перехода — обычные сутки)
{
  // США: 8.03.2026 и 1.11.2026; Европа: 29.03.2026 и 25.10.2026; Лорд-Хау: 5.04 и 4.10.2026 (сдвиг 30 мин)
  for (const [y, mo, d] of [[2026, 3, 8], [2026, 11, 1], [2026, 3, 29], [2026, 10, 25], [2026, 4, 5], [2026, 10, 4]]) {
    const c = E.currentCycle(11, L(y, mo, d, 8, 0));
    ok(c && new Date(c.task).getHours() === 6 && new Date(c.task).getMinutes() === 1, `летнее время ${d}.${mo}: задача в 6:01`);
    ok(c && new Date(c.reset).getHours() === 11, `летнее время ${d}.${mo}: сброс в 11:00`);
    const nt = E.nextTask(3, L(y, mo, d, 12, 0));
    ok(new Date(nt).getHours() === 22 && new Date(nt).getDate() === d, `летнее время ${d}.${mo}: задача на сброс 3:00 — в 22:01 этого дня`);
  }
}

// 5. Котофакт: один в день, разный в соседние дни, облачко раз в день
{
  const a = E.factOfDay(FACTS, L(2026, 10, 30, 0, 5));
  const b = E.factOfDay(FACTS, L(2026, 10, 30, 23, 55));
  const c = E.factOfDay(FACTS, L(2026, 10, 31, 0, 5));
  ok(a && a === b, "весь день один факт");
  ok(a !== c, "назавтра новый факт");
  ok(FACTS.every((f) => f.length <= 200), "факты короче 200 символов");
  ok(new Set(FACTS).size === FACTS.length, "факты не повторяются");
  eq(E.dayKey(L(2026, 1, 5, 23, 59)), "2026-01-05", "ключ дня");
  const now = L(2026, 10, 30, 10, 0);
  ok(E.showFact({ enabled: true, cat: true, hiddenDay: null, now }), "облачко показываем");
  ok(!E.showFact({ enabled: true, cat: true, hiddenDay: "2026-10-30", now }), "закрыли сегодня — не показываем");
  ok(E.showFact({ enabled: true, cat: true, hiddenDay: "2026-10-29", now }), "закрыли вчера — снова показываем");
  ok(!E.showFact({ enabled: true, cat: false, hiddenDay: null, now }), "без котика — без облачка");
  ok(!E.showFact({ enabled: false, cat: true, hiddenDay: null, now }), "котофакт выключен");
}

console.log(`[${tz}] ${n - bad}/${n} ок`);
process.exit(bad ? 1 : 0);
