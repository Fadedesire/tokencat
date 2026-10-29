# Контекст: источник данных, грабли, риски

Окружение Ромы и правила работы с ПК — в START.md.

## Источник данных (неофициальный, может поменяться)
- `GET https://claude.ai/api/organizations` → организация с `"chat"` в `capabilities`
  (сначала пробуем cookie `lastActiveOrg`).
- `GET https://claude.ai/api/organizations/{uuid}/usage`. Главное — массив `limits`:
  `{group, kind: session|weekly_all|…, percent, resets_at, is_active, severity, scope}`.
  Запасной вариант — `five_hour` / `seven_day` (`utilization` 0–100, `resets_at`), но
  `five_hour` бывает отстаёт. Есть `seven_day_breakdown.rows` (доля недели по продуктам,
  `display_name` уже локализован) и `spend` (кредиты сверх плана). Много новых ключей с
  кодовыми именами — игнорируем; любое окно может быть `null`. У 5-часового окна при 0 %
  может не быть `resets_at` → «окно не начато».
- Авторизация: cookie `sessionKey` (`sk-ant-sid…`, домен `.claude.ai`). Ещё бывают
  `sessionKeyV3`, `sessionKeyLC`, `__Host-sessionBinding*`, `cf_clearance` — нативному
  клиенту пока хватает `sessionKey`. Если сервис начнёт привязывать сессию к браузеру —
  включать запасной путь (fetch в скрытом webview, см. DECISIONS S02).
- Заголовки: `Accept: application/json`, `Referer: https://claude.ai/`,
  `anthropic-client-platform: web_claude_ai`, браузерный User-Agent.

## Грабли прототипа (Python/Tk, папка `prototype/`, только образец)
1. Cookie браузеров и Claude Desktop не читать — это чужие защищённые хранилища.
2. 403 от Cloudflare ≈ регион/VPN, а не протухшая сессия.
3. Русская раскладка ломала Ctrl+V — проверять вставку в новых полях ввода.
4. Сборка должна содержать все зависимости — проверять собранный exe, а не «из исходников».
5. Ошибка говорит, что делать; подробности — в лог.
6. PowerShell 5 `Tee-Object` пишет UTF-16; наши логи пишет `tools\run-logged.ps1` в UTF-8.

## Риски для продаж
- Условия Anthropic запрещают доступ ботами/скриптами без разрешения и использование их
  имени и логотипа. Поэтому свой бренд, пометка «неофициально, не связано с Anthropic»,
  щадящий опрос, никакого обхода защит, письмо в Anthropic (B6), модель «бесплатно + донаты».
- Источник может сломаться в любой день → быстрый хотфикс через автообновление (B4, B8).

## Конкуренты (октябрь 2026)
Бесплатные трекеры в основном под macOS (меню-бар) или расширения Chrome/VS Code. Под Windows —
платный OhNine (9 €, Electron). Наши отличия: нативные виды на обеих ОС, панель задач Windows,
вход в один клик, котик, вес.
