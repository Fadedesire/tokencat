# Code signing policy

Windows builds of TokenCat are not signed yet. We have applied to the SignPath Foundation
free code signing program for open source. Once accepted, this page will read:

> Free code signing provided by [SignPath.io](https://about.signpath.io),
> certificate by [SignPath Foundation](https://signpath.org).

## What gets signed

Only binaries built by GitHub Actions from this repository ([ci/release.yml](../ci/release.yml))
from a `v*` tag: the NSIS installer and `TokenCat.exe`. Nothing built on a personal machine
is ever signed or published.

## Team roles

| Role | Who |
|---|---|
| Author, reviewer, approver | [@Fadedesire](https://github.com/Fadedesire) |

All maintainers use multi-factor authentication on GitHub (and SignPath).

## Privacy

TokenCat connects only to `claude.ai` (to read the signed-in user's usage limits) and to
`github.com` (to check for updates once a day). It sends no analytics or telemetry. The
claude.ai session is stored in Windows Credential Manager / macOS Keychain and is never sent
anywhere else. See [README](../README.md#privacy).

## Updates

Self-updates are verified with a separate minisign key (Tauri updater); an update whose
signature does not match the public key in `tauri.conf.json` is refused.
