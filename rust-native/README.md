# Mission Control: pure Rust edition

The same widget with **no web view at all**: the interface is drawn natively on the GPU with
[egui](https://github.com/emilk/egui) (OpenGL), and everything behind it is Rust. It looks and
behaves like the other editions: the same notch, pet and moods, tabs, charts, controls, chat, cleaner
and Extreme mode, with the same sizes, colours, easing curves and timings (ported from
`renderer/styles.css` and `renderer/app.js`). It adds a **Credits** tab listing the project's GitHub
contributors.

| | Electron edition | Pure Rust edition |
| --- | --- | --- |
| Download / install size | ~300 MB | 4.1 MB installer, 7 MB app |
| Memory while idle (Windows) | ~364 MB in 4 processes | ~87 MB in 1 process |
| CPU while idle (Windows, one core) | ~10% | ~3% |

Measured on one Windows 11 PC (i5-12400, RTX 3060), widget collapsed, same settings.

## Install

- **Windows:** run `installers/Mission Control Native_1.0.0_x64-setup.exe`. It installs for your
  user account only (no admin rights). SmartScreen may warn because the installer isn't code-signed:
  click **More info**, then **Run anyway**.
- **macOS (Apple Silicon):** download [`Mission Control Native_1.0.0_aarch64.dmg`](../installers/Mission%20Control%20Native_1.0.0_aarch64.dmg), open it and drag **Mission Control Native** into Applications. This is the pure Rust/egui build, distinct from the older Tauri installer named `Mission Control_...dmg`. The local build is ad-hoc signed and is not Apple-notarized; macOS may require approval in Privacy & Security before first use.
- **Intel Mac:** build locally on an Intel Mac with the script below; no tested Intel installer is included.

Run only one edition at a time: they all sit in the same spot at the top of the screen, share one
settings file and use the same "launch at startup" entry.

## Extreme mode dock (Windows)

On Windows, Extreme mode themes the Start menu and accent colours like the other editions and, in
place of the full-width taskbar, shows a centred **Mission Control dock**:

- Start button, your taskbar's pinned apps (same order), then other running apps after a divider.
- macOS-style magnification under the pointer, the app's name above it, a launch bounce, and a cyan
  dot under running apps (a wider bar for the app in front).
- Click: open, switch to, or minimise the app in front (cycles through an app's windows).
  Middle-click: a new window. Right-click: New window / Close window.
- The Windows taskbar is set to auto-hide while the dock is on, so the tray, clock and notifications
  slide up at the bottom edge as usual (the dock steps aside meanwhile). The dock reserves its strip
  like the taskbar did, so maximised windows end above it.
- Turning Extreme mode off, or quitting, restores your taskbar setting and colours exactly.

## Build from source

Prerequisites are the same as for the Tauri edition (Rust via rustup; on Windows the Visual Studio
Build Tools with *Desktop development with C++*; on macOS the Xcode command line tools for the small
Swift helper). Node.js is **not** needed.

```powershell
cd rust-native
cargo run --release
```

Installers use [cargo-packager](https://github.com/crabnebula-dev/cargo-packager):

```powershell
cargo install cargo-packager --locked
cargo packager --release          # Windows: dist\mission-control_1.0.0_x64-setup.exe
```

```bash
./scripts/package-macos.sh   # macOS: signed app inside dist/Mission Control Native_<version>_<arch>.dmg
```

The local packaging script uses only Cargo and macOS's bundled signing/disk-image tools. It stages the app outside synced folders, includes the Swift helper and icon, verifies signatures and architecture, and verifies the disk image. It does not need a GitHub Actions workflow or Node.js. `packager-macos.json` remains available for cargo-packager users.

### Test a Mac build

Mount the generated DMG, then run:

```bash
python3 scripts/test-macos.py "/Volumes/Mission Control Native/Mission Control Native.app" --report-dir /tmp/mission-control-native-test
```

This verifies the bundle and six read-only native helper queries, captures all UI pages, and checks a normal timed exit. It uses an isolated temporary settings directory and disables startup registration and system theming. It does not exercise destructive actions, send AI prompts, run a speed test or install the app. Apple Silicon was built and tested on macOS 27.0.1; older macOS releases and Intel hardware have not been run-tested.

## How it is built

- `../rust/src-tauri/src/*.rs` (stats, network and speed test, processes, cleaner, Claude Code and
  Codex bridges, system controls, Extreme mode, launch at startup) are **shared** with the Tauri
  edition and compiled unchanged. They only use three things from Tauri (emit an event, spawn a task,
  the `AppHandle` type); `host/` provides those under the name `tauri`, so a fix in one edition's
  backend is a fix in both.
- `src/notch.rs`, `pages.rs`, `controls.rs`, `chat.rs`, `credits.rs` draw the interface. `draw.rs`
  gives egui what the CSS used (gradients clipped to rounded shapes, glows, letter spacing, the inline
  SVG icons as vectors) and `anim.rs` gives it CSS transition semantics with the same cubic-bezier
  curves.
- The window is transparent and lets clicks through except over the widget itself. A pointer thread
  (`pointer.rs`) watches the cursor, like Electron's forwarded mouse moves, and wakes the interface
  only when something visible should change. While nothing moves, nothing is redrawn; small background
  animations (the bar meters) run at 30 fps, everything you interact with at 60 fps.
- Extreme mode's dock (`dock.rs`) is a layered window drawn on the CPU (tiny-skia) from its own
  thread, so it needs no second GPU context, and it only redraws while something moves.
- macOS uses the same Swift helper as the other editions for CoreAudio, CoreWLAN and Bluetooth
  (`build.rs` compiles it; the packager puts it in the app's Resources).

## Development switches

| Variable | Effect |
| --- | --- |
| `MC_SETTINGS_DIR=<folder>` | Use another settings folder (test runs never touch your real settings) |
| `MC_NO_LOGIN=1` | Don't touch the launch-at-startup entry |
| `MC_DIAG=1` | Never change Windows colours, the taskbar or startup (Extreme mode still shows the dock, without reserving space) |
| `MC_SHOTS=<folder>` | Save a PNG of the collapsed notch, hover, every tab and the chat, then stay open (`MC_SHOTS_ONLY=overview,chat` limits it) |
| `MC_DEV_OFFSET_X=<px>` | Shift the window sideways, to test next to an installed copy |
| `MC_NO_AI=1` | Preview the layout without Claude Code / Codex |
| `MC_DEV_QUIT_AFTER=<s>` | Quit normally after that many seconds (tests that Extreme mode restores everything) |
| `MC_DOCK_DEBUG=1` | Log the dock's apps and how their icons resolved |
| `MC_DIAG_FPS=1`, `MC_DIAG_ANIM=1` | Log every frame / every running animation (to find what keeps the widget repainting) |
