# TokenCat 🐾

**Limit tracker for Claude (unofficial).** A tiny desktop app for Windows 10/11 and macOS 13+
that shows how much of your Claude subscription limits is used — the 5-hour window and the
week — and when they reset. With a pixel cat.

*Трекер лимитов для Claude (неофициальный): показывает, сколько израсходовано из 5-часового
и недельного лимита подписки и когда сброс. Русский — ниже.*

<p align="center">
  <img src="marketing/en-1-panel.png" width="49%" alt="Floating panel with the cat fact of the day">
  <img src="marketing/2-taskbar.png" width="49%" alt="Windows taskbar widget">
  <img src="marketing/en-3-cat-worried.png" width="49%" alt="The cat gets worried at 80 %">
  <img src="marketing/en-4-settings.png" width="49%" alt="Settings">
</p>

- Sign in once in a built-in claude.ai window — no API keys to copy.
- Three looks: floating panel, Windows taskbar widget, macOS menu bar.
- Notifications at 80 % / 100 % and when the limit resets.
- 9 languages, ~100 MB RAM, a few MB on disk, updates itself.
- Free. Your session stays in Windows Credential Manager / macOS Keychain and is sent only to
  claude.ai.

## Download

[Latest release](https://github.com/Fadedesire/tokencat/releases/latest):

| System | File |
|---|---|
| Windows (installer, auto-updates) | `TokenCat_<version>_x64-setup.exe` |
| Windows (portable, no install) | `TokenCat_<version>_x64-portable.zip` |
| macOS 13+ (Apple Silicon and Intel) | `TokenCat_<version>_universal.dmg` |

The app is not code-signed yet, so the first launch shows a warning —
see [how to open it](docs/INSTALL.md).

## Privacy

TokenCat talks only to `claude.ai` (your usage numbers) and to GitHub (update check, once a
day). No analytics, no telemetry. The log file masks your session key.

## Build from source

Rust (stable) + Node.js LTS, then in `app/`: `npm install` and `npx tauri build`.
Windows helpers: `dev.bat`, `test.bat`, `build.bat`. Releases are built by
[GitHub Actions](ci/release.yml) — see [docs/RELEASE.md](docs/RELEASE.md).

## Support

TokenCat is free. If it saves you a surprise "limit reached", you can
[buy the cat a coffee](DONATE_LINK) ☕. Bugs and ideas — [Issues](https://github.com/Fadedesire/tokencat/issues).

## License

[MIT](LICENSE). TokenCat is not affiliated with, endorsed by or sponsored by Anthropic.
"Claude" is a trademark of Anthropic, PBC.

---

## По-русски

<p align="center">
  <img src="marketing/ru-1-panel.png" width="49%" alt="Плавающая панель с котофактом дня">
  <img src="marketing/ru-4-settings.png" width="49%" alt="Настройки">
</p>

Маленькая программа для Windows и macOS: в реальном времени показывает, сколько
израсходовано из 5-часового и недельного лимита подписки Claude и когда сброс. Вход один раз
во встроенном окне claude.ai, без ключей. Плавающая панель, виджет в панели задач Windows
или строка меню macOS, уведомления, котик, 9 языков. Бесплатно, обновляется сама.

Скачать — в разделе [Releases](https://github.com/Fadedesire/tokencat/releases/latest).
Программа пока без цифровой подписи, поэтому при первом запуске Windows/macOS предупредит —
[как открыть](docs/INSTALL.md).

Поддержать котика — [Ko-fi](DONATE_LINK). Ошибки и идеи — [Issues](https://github.com/Fadedesire/tokencat/issues).

Неофициальная программа, не связана с Anthropic.
