# WinNas Launcher

> A **TV launcher** designed for Windows x86 mini PCs + projector/TV setups, turning Windows into an Android TV-like media center desktop.

Boots straight into a fullscreen app grid; navigate with your TV remote / keyboard arrow keys and launch your Kodi, Jellyfin, player, games and more — no keyboard or mouse needed.

**Language:** [English](README.en.md) | [中文](README.md)

## ✨ Features

- 🖥️ **Borderless exclusive fullscreen** — boots straight to the fullscreen, always-on-top launcher without ever seeing the Windows desktop
- 🎮 **Remote / keyboard only** — arrow-key focus navigation, paged grid, apps auto-sorted by usage frequency
- 🚀 **Launch any program in one click** — supports `.exe` / `.lnk`; the app stays on top after launch and you automatically return to the launcher when it exits
- 📺 **Grid menu + paging** — 6×4 grid with page flipping and auto-extracted app icons
- 🗂️ **App management** — remove, move to front/back (pinned ordering), persisted across restarts
- 🔒 **System shortcut interception** — blocks accidental Win / Win+D / Alt+F4 etc. from switching away from the launcher
- 🔊 **Volume control + OSD** — adjust system volume with the remote's volume keys, with on-screen OSD feedback
- ⏻ **System control** — restart / sleep / lock / exit
- 🛡️ **Crash self-recovery** — auto-restart on crash + system-state snapshot self-healing (auto-restores taskbar/desktop on next boot after a force kill)
- 💾 **Two distribution forms** — regular installer + portable version (no registry writes, no APPDATA writes)

## 🖥️ Interface

- **Home** — centered 6×4 app grid (paged), clock + settings gear in the top-right corner
- **Settings drawer** — restore desktop, clear menu cache, autostart on boot, add APP, keymap, system operations, exit, maintenance mode
- **First-run onboarding** — choose "Load all menus" (scan Start Menu programs) or "Don't load menus" (add manually)

  ![docs/sample.png](docs/sample.png)

## 📦 Download & Install

> **Latest release v0.1.1**:
>
> | Version | Download |
> |------|------|
> | Portable (recommended, extract and run) | [WinNas-Launcher-portable.zip](https://github.com/hanb102400/winnas-launcher/releases/download/v0.1.1/WinNas-Launcher-portable.zip) |
> | Regular installer | [WinNas.Launcher_0.1.1_x64-setup.exe](https://github.com/hanb102400/winnas-launcher/releases/download/v0.1.1/WinNas.Launcher_0.1.1_x64-setup.exe) |
>
> Past releases on the [Releases page](https://github.com/hanb102400/winnas-launcher/releases).

### Option 1: Portable version (recommended)

1. Download `WinNas-Launcher-portable.zip` and extract it
2. Double-click `winnas-launcher.exe` to run

The portable version **writes nothing to the registry or %APPDATA%**; config/logs live in a `conf/` folder next to the program. A `portable.flag` file is bundled to enable portable mode.

### Option 2: Regular installer

1. Download `WinNas.Launcher_0.1.1_x64-setup.exe`
2. Double-click to install

The installer version stores config in `%APPDATA%\WinNasLauncher\conf\`. If WebView2 Runtime is missing it will prompt to download it online (Win10/11 usually has it built in).

> Requirements: Windows 10 / 11 (x64) with WebView2 Runtime.

## 🕹️ Usage

| Key | Function |
|------|------|
| ↑ ↓ ← → | Move focus / flip page (press → at the rightmost item to go next page, ← at the leftmost to go previous) |
| Enter | Confirm / launch app |
| Esc / Backspace | Back / exit confirmation |
| F1 / Menu key | Open settings |
| Volume + / - / Mute | Adjust system volume (with OSD) |

**First launch**: a dialog asks how to initialize the menu —
- **Don't load menus**: start with an empty grid, add apps manually via Settings → "Add APP"
- **Load all menus**: auto-scan Start Menu programs (uninstall/help/repair entries filtered out)

**Add APP**: Settings → "Add APP" → pick from the scanned installed-programs list, or type a program path manually (exe/lnk).

**Exit**: press Esc on the home screen → confirm exit; or Settings → "Exit Launcher".

**Maintenance mode**: Settings → "Enter maintenance mode" → restores the taskbar and lets the launcher step aside so you can work with the Windows desktop.

## 🛠️ Development & Build

```bash
# Requirements
# Node ≥ 20, Rust stable, Tauri v2 CLI
# Windows SDK + VS Build Tools (C++)

# Install dependencies
npm install

# Local development (desktop window debugging)
npm run tauri dev

# Package (NSIS installer + release exe)
npm run tauri build
```

The portable zip is assembled manually from `target/release/winnas-launcher.exe` + `portable.flag`.

## 🏗️ Tech Stack

- **Frontend**: React + TypeScript + Vite
- **Shell**: Tauri v2 (WebView2)
- **System integration**: Rust `windows` crate (Win32 APIs wrapped in `src-tauri/src/win/`)
  - Fullscreen always-on-top window, taskbar hiding, focus state machine, low-level keyboard hook, Job Object process trees, Core Audio volume, icon extraction, program scanner, config/logging

## 📁 Project Structure

```
winnas-launcher/
├── src/                    # React frontend (grid navigation, settings drawer, dialogs, OSD)
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs          # IPC command registration + startup flow
│   │   └── win/            # Win32 system wrapper modules
│   │       ├── window.rs   # fullscreen always-on-top
│   │       ├── focus.rs    # focus state machine
│   │       ├── keyhook.rs  # keyboard hook
│   │       ├── process.rs  # process launch/exit detection
│   │       ├── volume.rs   # volume control
│   │       ├── scanner.rs  # program scanning
│   │       ├── icon.rs     # icon extraction
│   │       ├── config.rs   # config/menu persistence
│   │       ├── system_state.rs  # system-state snapshot self-healing
│   │       └── ...
│   └── tauri.conf.json
├── poc/                    # M0 technical validation projects (5 PoCs)
└── docs/                   # design docs, acceptance checklist, packaging notes
```

## ❓ FAQ

**Q: The launched app window isn't brought to the front?**
Fixed: `.lnk` launches go through `ShellExecuteExW` + `SEE_MASK_NOCLOSEPROCESS` to capture the PID and activate the window.

**Q: Does Alt+Tab switching to other programs work normally?**
Yes. The always-on-top follows focus: the launcher is pinned while focused and unpinned when you switch away, so it never blocks other programs.

**Q: How do I do a full reset?**
Settings → "Clear desktop menu cache" → exit; the first-run onboarding runs again on next launch.

**Q: Where are the logs?**
Portable: `conf/logs/YYYY-MM-DD.log`. Installer: `%APPDATA%\WinNasLauncher\conf\logs\`.

**Q: Can't wake the device with the remote after sleep?**
Sleep uses system S3 suspend with wake events enabled. If the remote can't wake it, check in this order:

1. Check whether **BIOS** has "USB Wake from S3 / Resume by USB" enabled;
2. Open "Device Manager" → find the remote receiver (USB input device / Bluetooth / HID) → right-click → Properties → "Power Management" tab → check "**Allow this device to wake the computer**";
3. Run `powercfg /a` to confirm the system supports **S3** (if only S0 Modern Standby is available, that's an architecture decision and S3 can't be forced).

## 🧩 Recommended Companion Apps

These Windows apps **natively support full remote / gamepad operation** — no keyboard or mouse needed — and pair best with WinNas Launcher for a TV experience:

**Media Centers / Players**

| App | Notes |
|------|------|
| [Kodi](https://kodi.tv) | Open-source media center with the best remote experience; local/network libraries, rich plugin ecosystem |
| [MediaPortal 2](https://www.team-mediaportal.com) | Veteran native Windows media center with native MCE infrared remote support; movies/series/music/live TV DVR built in |
| [Jellyfin Media Player](https://jellyfin.org) | Open-source media client that pairs with a Jellyfin server; hardware decoding |
| [Plex HTPC](https://www.plex.tv) | Media server client with a big-screen TV mode |
| [Emby Theater](https://emby.media) | Media client with remote navigation |
| [JRiver Media Center](https://jriver.com) | All-in-one media manager; Theater View pairs perfectly with a remote; favorite of audio/video enthusiasts (paid) |
| [MPC-BE](https://sourceforge.net/projects/mpcbe) / MPC-HC | Lightweight media players; can be mapped to the remote |
| [Zoom Player MAX](https://inmatrix.com/zplayer/) | Veteran HTPC player with theater mode + remote support; comprehensive formats/subtitles/filters |
| [TinyPlay](https://github.com) | Open-source lightweight media frontend; SMB/NFS local video + IPTV, fully remote-controlled, friendly to low-end mini PCs |
| [VLC](https://www.videolan.org/vlc) | General-purpose player with fullscreen mode |

**Game Platforms / Libraries**

| App | Notes |
|------|------|
| [Steam](https://store.steampowered.com) | Big Picture mode natively supports gamepad/remote; integrated game library |
| [Playnite](https://playnite.link) | Open-source game library aggregator with fullscreen mode; unified multi-platform game management |
| [LaunchBox + BigBox](https://www.launchbox-app.com) | BigBox is a standalone fullscreen TV frontend with gorgeous box-art walls; aggregates PC games + emulators; perfect with IR/Bluetooth remotes (paid) |
| [GOG Galaxy](https://www.gog.com/galaxy) | Game platform with big-screen mode |
| Xbox App (Microsoft Store) | Fullscreen controller mode; browse Game Pass / first-party titles with arrow keys (limited to Microsoft games) |

**Game Streaming**

| App | Notes |
|------|------|
| [Moonlight](https://moonlight-stream.org) | Streams PC games (paired with a Sunshine server); gamepad/remote |
| [Parsec](https://parsec.app) | Low-latency game streaming |

**Emulators**

| App | Notes |
|------|------|
| [RetroArch](https://www.retroarch.com) | Multi-platform emulator frontend with tons of cores; XMB/Ozone interfaces navigable by gamepad/remote |
| [EmulationStation DE](https://es-de.org) | Open-source, free emulator-only frontend; fullscreen box-art wall; native remote support (free LaunchBox alternative) |
| [RetroBat](https://www.retrobat.org) | Windows emulation frontend with a built-in EmulationStation UI and auto-configured RetroArch + standalone emulators; fullscreen box-art wall, native remote/gamepad support |
| [Dolphin](https://dolphin-emu.org) | GameCube / Wii emulator |
| [PCSX2](https://pcsx2.net) | PS2 emulator |
| [RPCS3](https://rpcs3.net) | PS3 emulator |
| [DuckStation](https://www.duckstation.org) | PS1 emulator |
| [PPSSPP](https://www.ppsspp.org) | PSP emulator |
| [MAME](https://www.mamedev.org) | Arcade emulator |

**Streaming / Live TV**

| App | Notes |
|------|------|
| Netflix (Microsoft Store) | Remote-friendly big-screen viewing |
| YouTube (Microsoft Store client) | The Store client has native focus navigation (unlike the web version); browse videos with a remote |
| Prime Video / Disney+ / Apple TV+ (Microsoft Store) | Remote-friendly |
| Twitch (Microsoft Store TV client) | Big-screen UI; browse streams and search fully with a remote |
| Bilibili UWP (Microsoft Store) | The new UWP supports focus navigation and works with a remote (the web version doesn't) |
| [Stremio](https://www.stremio.com) | Streaming aggregator with a big-screen box-art UI; remote basically usable (not as fully remote-driven as Kodi) |
| [ProgTV / ProgDVB](https://www.progdvb.com) | IPTV, network TV, TV tuner cards; big-screen TV UI with native remote support |
| [HDHomeRun](https://www.silicondust.com) | TV tuner live TV |
| [NextPVR](https://nextpvr.com) | Personal video recording / live TV |

**Music**

| App | Notes |
|------|------|
| [Spotify](https://www.spotify.com) | Music playback with big-screen mode |
| [Plexamp](https://plexamp.com) | Plex music player |
| [Deezer](https://www.deezer.com) | Music streaming |
| [MusicBee](https://getmusicbee.com) | Local music library manager; enable Theater Mode to browse album covers with a remote |

> Add any of these apps to the WinNas Launcher home grid and launch them with one remote press, returning to the launcher automatically when they exit. For emulators, a gamepad is recommended (WinNas Launcher already supports gamepad input).

## 📄 License

MIT License (free to modify).
