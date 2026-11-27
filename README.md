# Mission Control

A futuristic system widget for Windows that hangs from the top-center of your screen like a notch,
with an animated robot pet, live PC stats, device controls, internet speed tests, a cache cleaner,
and a built-in AI chat that can control your PC through
[Claude Code](https://claude.com/claude-code) or the OpenAI Codex CLI, whichever you have.

![The notch](docs/screenshots/notch.png)

## Features

- **Always-on notch**: CPU, GPU and RAM on the left, live download/upload speed and your AI usage
  limits on the right (Claude Code 5-hour and weekly; Codex windows as your plan reports them). With
  both installed, the usage chip alternates between them. With neither installed, the right side and
  the chat disappear and the notch re-centres around the stats and the pet.
- **Animated pet**: its eyes follow your cursor and it blinks. It is happy when you hover over it,
  sweats and turns red when CPU or RAM is maxed out, spins an orbit ring while Claude works, gets
  excited during speed tests and falls asleep when you're away. It also shows speech bubbles for
  important events (high load, a nearly full disk, connection lost or back, Claude finished).
- **Mission Control panel** (click the pet):
  - **Overview**: CPU / RAM / GPU / VRAM gauges, GPU temperature and power, a load history graph,
    per-thread load, storage per drive, uptime and system info.
  - **Controls**: one place for your PC's inputs, outputs and toggles, synced live with Windows:
    - Wi-Fi and Bluetooth on/off, with Wi-Fi networks (connect to saved ones) and paired Bluetooth
      devices with their connection status
    - Sound and microphone mute, volume and mic level sliders, and switching the default output
      or input device
    - Monitor brightness over DDC/CI (laptop panels through WMI)
    - Dark mode, the "now playing" media session with play/pause/next/previous and a visualiser
    - Power plan switching, Lock, Sleep, Restart and Shut down (the last three ask you to confirm)
    - Shortcuts to the matching Windows Settings pages
  - **Network**: a live transfer graph, constant ping / jitter / packet-loss monitoring, a speed
    test you start by hand (download, upload, ping, jitter, loss) and test history.
  - **Apps**: running apps and background processes grouped by executable and sorted by memory,
    with "End task" (critical system processes are protected).
  - **Cleaner**: scans user and Windows temp files, Chrome / Edge / Brave / Firefox caches, GPU
    shader caches, crash dumps, thumbnail cache, npm / pip caches, Windows Update downloads and the
    Recycle Bin, then deletes what you select after you confirm.
  - **Settings**: launch at startup, idle during fullscreen, collapse on click-away, Claude
    permission mode, and auto-refresh of Claude limits.
- **AI chat** (right-click the pet): ask Claude Code or Codex to open apps, find files, change
  settings, explain what is using your RAM, and so on. Every command the AI runs shows as a chip.
  With both installed, switch between them in the chat header.
- **Energy friendly**: while any app is fullscreen (games, videos, presentations) the widget hides
  and stops all sampling. When folded up, the window shrinks to just the notch and the pet animates
  only in short bursts.
- **Fluid motion**: spring-based morphing between notch, panel and chat, smooth scrolling charts
  and tweened numbers.

| Mission Control | Network |
| --- | --- |
| ![Overview](docs/screenshots/overview.png) | ![Network](docs/screenshots/network.png) |

| Claude Code chat | Cleaner |
| --- | --- |
| ![Chat](docs/screenshots/chat.png) | ![Cleaner](docs/screenshots/cleaner.png) |

## Requirements

- **Windows 10 or 11** (x64). The widget uses Win32 APIs, so it does not run on macOS or Linux.
- **Node.js 18+** and npm.
- **Optional:** an **NVIDIA GPU** with `nvidia-smi` (it ships with the driver) for GPU / VRAM /
  temperature stats. Other GPUs show "No NVIDIA data".
- **Optional:** the **Claude Code CLI** and/or the **OpenAI Codex CLI**, installed and signed in,
  for the chat and usage limits. Everything else works without either, and anything for a CLI that
  isn't installed (its usage chip, chat option and settings) stays hidden.
  - Claude Code is found from the native installer (`~/.local/bin`), `PATH`, WinGet or npm.
  - Codex is found on `PATH` (npm, WinGet, Scoop, standalone) or in npm's global folder. The copy
    bundled inside the OpenAI extension for VS Code / Cursor / Windsurf is **not** used unless you
    turn on **Settings → Use Codex from your code editor** (shown only when such a copy exists).

## Install and run

```powershell
git clone https://github.com/Shadhin-f/mission-control-widget.git
cd mission-control-widget
npm install
npm start
```

The notch appears at the top-center of your main display, and a tray icon appears next to the clock.

On first launch, Mission Control registers a **Task Scheduler "at log on" task** (no admin rights
needed) so it **starts as soon as you sign in**. Run-key entries and Startup-folder shortcuts wait
behind every other startup app, which can take minutes on a busy PC. If the task can't be created,
it falls back to a Startup-folder shortcut. You can turn this off in **Settings → Launch at
startup** or from the tray menu. If you move the folder, launch it once from the new place and the
task updates itself.

### Troubleshooting

- **`npm start` says Electron failed to install / `electron.exe` is missing.** The Electron
  download sometimes gets skipped. Run:
  ```powershell
  node node_modules/electron/install.js
  ```
- **GPU shows "No NVIDIA data".** `nvidia-smi` was not found on `PATH`. This is expected on
  AMD / Intel GPUs.
- **Chat says "Claude Code CLI not found".** Install Claude Code and run `claude` once in a
  terminal to sign in.
- **Some cleaner items free less than expected.** Files that are in use (an open browser, Explorer
  thumbnails) are skipped. Close the app and clean again. Items tagged **ADMIN** need the widget to
  run as administrator to be cleaned fully.

## Using it

| Action | How |
| --- | --- |
| Open Mission Control | Click the pet, press `Ctrl+Alt+M`, or click the tray icon |
| Ask Claude | Right-click the pet, press `Ctrl+Alt+Space`, or hover the notch and click **Ask Claude** |
| Collapse | `Esc`, the ˄ button, or click anywhere else (use the pin button to keep it open) |
| Refresh Claude limits | Click the Claude usage chip on the right of the notch |
| Change volume from anywhere | Scroll over the pet |
| Quit | **Settings → Quit**, or the tray menu |

![Hover actions](docs/screenshots/hover.png)

### Claude Code chat

The chat runs your locally installed Claude Code CLI (`claude -p` with streaming JSON output) using
**your existing Claude Code login**. Nothing is sent anywhere except through the CLI itself. One
conversation continues until you press **New conversation**.

Choose the permission mode in the chat header or in Settings:

| Mode | What Claude can do |
| --- | --- |
| **Auto** (default) | Runs safe actions itself; risky actions are blocked and listed in the chat |
| **Accept edits** | Can also edit files without asking |
| **Plan** | Read-only: looks and explains, changes nothing |

Claude is told to describe any destructive action and wait for your confirmation in the chat first.

### Codex chat and limits

- Chat runs `codex exec --json` in your home folder and continues the conversation with
  `codex exec resume`. Auto and Accept edits use Codex's `workspace-write` sandbox; Plan uses
  `read-only`.
- If the model in `~/.codex/config.toml` isn't available on your account, the widget picks the
  first model Codex lists for you. Your config file is never changed.
- Limits come from `codex app-server` (`account/rateLimits/read`), which costs no usage, so they
  refresh every 5 minutes and after every Codex reply. Paid plans (Plus, Pro, Business) show the
  **5-hour** and **weekly** windows, like Claude; the free plan only has a single 30-day window.
- Older Codex versions without that method fall back to the newest usage event Codex writes to
  `~/.codex/sessions` (only its `rate_limits` field is read).

### Claude usage limits (5h / 7d)

The right-hand chip shows how much of your Claude plan's **5-hour** and **weekly** limits you have
used. Hover over it to see when each one resets. The numbers come from the rate-limit information the
Claude Code CLI reports with every reply:

- Every message in the widget chat updates them at no extra cost.
- While a 5-hour window is already open, the widget refreshes every 10 minutes with a minimal
  Claude Code request (Haiku, no tools, a two-word prompt). It never starts a new window on its own.
  You can turn this off in Settings.
- Click the chip to refresh right away.

## Privacy

Mission Control has no telemetry, accounts or analytics. It only makes these network requests:

| What | Where | When |
| --- | --- | --- |
| Latency / packet-loss monitor | ICMP echo to `1.1.1.1` (TCP 443 fallback) | Every 3 s while visible |
| Speed test | `speed.cloudflare.com` | Only when you press **Run speed test** |
| Claude chat and usage limits | Through your local Claude Code CLI | When you chat, refresh, or auto-refresh is on |
| Codex chat and usage limits | Through your local Codex CLI | When you chat, and every 5 min for limits |

Settings (including speed-test history and the last known Claude limits) are stored only on your
PC in `%APPDATA%\Mission Control\settings.json`.

## How it works

```
main.js              Electron main: transparent always-on-top window, tray, shortcuts,
                     fullscreen idle loop, login item, IPC
preload.js           The small, safe API exposed to the UI (contextIsolation on)
src/native.js        Win32 via koffi: fullscreen detection, app windows, network
                     counters (GetIfTable2), native ICMP ping, Recycle Bin
src/stats.js         CPU per thread, RAM, disks, network rates, nvidia-smi stream
src/network.js       Live latency monitor + Cloudflare speed test
src/processes.js     tasklist + window enumeration, end task
src/cleaner.js       Cache / junk scanner and cleaner
src/claude.js        Claude Code CLI bridge (chat streaming + usage limits)
src/codex.js         Codex CLI bridge (detection, chat + resume, app-server usage limits)
src/syscontrol.js    Controls tab backend: drives sysctl.ps1, netsh (Wi-Fi), powercfg
src/sysctl.ps1       Long-lived helper (started on first use, stops when idle): Core Audio,
                     Wi-Fi/Bluetooth radios, paired devices, media session, DDC/CI
                     brightness, dark mode, lock / sleep
src/settings.js      JSON settings in the user data folder
renderer/            UI: index.html, styles.css, app.js (pet, charts, tabs, chat)
```

- **Click-through:** the window is transparent and ignores the mouse everywhere except on the
  widget itself, so the desktop behind stays usable.
- **Idle mode:** every 1.5 s it checks whether the foreground window covers its whole monitor (or
  Windows reports a D3D fullscreen / presentation state). If so, the widget hides and stops sampling.

### Development

```powershell
npm start
```

Use **Reload widget** in the tray menu to pick up UI changes. There are two dev-only environment
variables:

- `MC_SHOTS=<folder>` renders every view to PNG files in that folder (used for the screenshots
  above). `MC_TABS=overview,network` limits which tabs are captured.
- `MC_DIAG=1` logs running CSS animations, which helps keep idle GPU use low.
- `MC_NO_AI=1` pretends neither Claude Code nor Codex is installed, to preview that layout.
- `MC_DEMO_CODEX=1` (with `MC_SHOTS`) previews the usage chip with sample paid-plan Codex numbers.

Dev runs with these variables never touch the startup task, so a test copy can't take over
launch-at-sign-in from your real install.

## Contributing

Issues and pull requests are welcome. Good first ideas:

- AMD / Intel GPU stats (for example through Windows performance counters)
- Multi-monitor placement options
- More pet moods and skins
- Packaging into a signed installer (electron-builder)

Please keep the widget light: avoid always-running animations and timers while it is collapsed.

## License

[MIT](LICENSE)

Not affiliated with Anthropic. "Claude" and "Claude Code" are trademarks of Anthropic.
