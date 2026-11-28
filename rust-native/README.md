# Mission Control: pure Rust edition

The same widget with **no web view at all**: the interface is drawn natively on the GPU with
[egui](https://github.com/emilk/egui) (OpenGL), and everything behind it is Rust. It looks and
behaves like the other editions: the same notch, pet and moods, tabs, charts, controls, chat, cleaner
and Extreme mode, with the same sizes, colours, easing curves and timings (ported from
`renderer/styles.css` and `renderer/app.js`). It adds a **Credits** tab listing the project's GitHub
contributors.

| | Electron edition | Pure Rust edition |
| --- | --- | --- |
| Download / install size | ~300 MB | 3.9 MB installer, 7 MB app |
| Memory while idle (Windows) | ~364 MB in 4 processes | ~87 MB in 1 process |
| CPU while idle (Windows, one core) | ~10% | ~3% |

Measured on one Windows 11 PC (i5-12400, RTX 3060), widget collapsed, same settings.

## Install

- **Windows:** run `installers/Mission Control Native_1.0.0_x64-setup.exe`. It installs for your
  user account only (no admin rights). SmartScreen may warn because the installer isn't code-signed:
  click **More info**, then **Run anyway**.
- **macOS:** the `.dmg` files (Apple Silicon and Intel) are built by the
  [Pure Rust edition workflow](../.github/workflows/rust-native.yml) on GitHub's Mac runners; download
  them from that workflow run's artifacts, open the dmg and drag **Mission Control Native** into
  Applications. The app isn't notarized: the first time, Control-click it and choose **Open**.

Run only one edition at a time: they all sit in the same spot at the top of the screen, share one
settings file and use the same "launch at startup" entry.

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
cargo packager --release -c "$(cat packager-macos.json)"   # macOS: .app and .dmg in dist/
```

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
- Extreme mode's glow line is a small layered window drawn on the CPU from its own thread (`edge.rs`),
  so it needs no second GPU context.
- macOS uses the same Swift helper as the other editions for CoreAudio, CoreWLAN and Bluetooth
  (`build.rs` compiles it; the packager puts it in the app's Resources).

## Development switches

| Variable | Effect |
| --- | --- |
| `MC_SETTINGS_DIR=<folder>` | Use another settings folder (test runs never touch your real settings) |
| `MC_NO_LOGIN=1` | Don't touch the launch-at-startup entry |
| `MC_DIAG=1` | Never change Windows colours or startup (Extreme mode still shows its glow line) |
| `MC_SHOTS=<folder>` | Save a PNG of the collapsed notch, hover, every tab and the chat, then stay open (`MC_SHOTS_ONLY=overview,chat` limits it) |
| `MC_DEV_OFFSET_X=<px>` | Shift the window sideways, to test next to an installed copy |
| `MC_NO_AI=1` | Preview the layout without Claude Code / Codex |
| `MC_DIAG_FPS=1`, `MC_DIAG_ANIM=1` | Log every frame / every running animation (to find what keeps the widget repainting) |
