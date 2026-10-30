# Installing TokenCat / Установка

TokenCat is not code-signed yet (we have applied for free signing for open source), so the
system warns about an "unknown publisher" the first time. This is expected for new free apps.
You can check the file against `SHA256SUMS-*.txt` on the release page.

## Windows

1. Run `TokenCat_<version>_x64-setup.exe`.
2. If you see **"Windows protected your PC"** (blue SmartScreen window): click
   **More info → Run anyway**.
3. The installer needs no admin rights and installs Microsoft WebView2 if it is missing.

Portable: unzip `TokenCat_<version>_x64-portable.zip` anywhere and run `TokenCat.exe` (same
SmartScreen step). The portable copy does not update itself — the app tells you when a new
version is out.

## macOS 13+

1. Open the `.dmg` and drag **TokenCat** to **Applications**.
2. First launch: **right-click TokenCat → Open → Open**.
   On macOS 15+: try to open it once, then **System Settings → Privacy & Security →
   Open Anyway**.
3. TokenCat lives in the menu bar (no Dock icon).

---

## Windows (по-русски)

1. Запустите `TokenCat_<версия>_x64-setup.exe`.
2. Синее окно **«Система Windows защитила ваш компьютер»** — нажмите **«Подробнее» →
   «Выполнить в любом случае»**. Так бывает у новых программ без платной подписи.
3. Права администратора не нужны; если нет WebView2, установщик поставит его сам.

Portable: распакуйте zip куда угодно и запустите `TokenCat.exe`. Сама она не обновляется —
программа подскажет, когда выйдет новая версия.

## macOS (по-русски)

Откройте `.dmg`, перетащите TokenCat в «Программы». Первый запуск: **ПКМ → Открыть →
Открыть**. На macOS 15+: попробуйте открыть, затем **Системные настройки → Конфиденциальность
и безопасность → «Всё равно открыть»**.
