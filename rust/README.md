# Mission Control: Rust edition

A second build of Mission Control with the backend rewritten in Rust on [Tauri 2](https://tauri.app).
It looks and behaves like the Electron edition because it **runs the same interface**: `../renderer`
is shared with it. A small bridge script (`src-tauri/src/bridge.js`) gives that interface the same
`window.mc` API that Electron's `preload.js` provides, and every call behind it is Rust.

It also has the features the [pure Rust edition](../rust-native/README.md) added, built from the same
Rust code where possible:

- **Power tab:** live power draw and what it costs per day, week, month and year. The CPU is read from
  Windows' energy meter, NVIDIA GPUs from the driver and laptop batteries from their charge rate;
  every other part is estimated, and each figure says which. Wall energy is logged per day in
  `power-log.json` next to the settings (the same file the pure Rust edition writes). It also offers
  a price, currency and adjustable estimates, plus calibration against a plug-in power meter.
- **Credits tab:** the project's GitHub contributors (cached for a day, avatars cached for offline use).
- **Smooth neon charts:** smoothed monotone-cubic lines with a glow and a hot core, a grid that scrolls
  with the data, a scan beam, hatching, pulsing heads and a peak marker. New samples glide in at the
  rate they arrive. The Electron edition shares these charts.
- **Extreme mode dock (Windows):** a centred Mission Control dock replaces the taskbar: Start, your
  pinned apps and running apps, magnification, names, launch bounce and running indicators. The
  taskbar auto-hides while it's on and the dock reserves its strip. Turning the mode off or quitting
  restores the taskbar setting, the work area and the colours.

The Power and Credits tabs appear only where the backend provides them, so the Electron edition keeps
its six tabs.

|                    | Electron edition         | Rust edition                              |
| ------------------ | ------------------------ | ----------------------------------------- |
| App on disk        | ~300 MB (bundled Chromium) | **5.4 MB** exe, uses the system WebView |
| System controls    | PowerShell helper + C#   | Direct Windows APIs from Rust             |
| Native calls       | koffi (FFI from Node)    | The `windows` crate                       |
| macOS native layer | Swift helper             | Same Swift helper, built by `build.rs`    |

## Requirements

- [Rust](https://rustup.rs) (stable) and Node.js 18+ (only for the Tauri CLI)
- **Windows:** Visual Studio Build Tools with "Desktop development with C++", and the WebView2
  runtime (built into Windows 11)
- **macOS:** the Xcode command line tools (`xcode-select --install`); `build.rs` compiles the Swift
  helper from `../src/macos/Helper.swift`

Step-by-step setup for both systems, with install commands and troubleshooting, is in the main
README under [Rust edition (Windows and macOS)](../README.md#rust-edition-windows-and-macos).

## Run and build

```sh
cd rust
npm install
npm run dev      # run the widget with live Rust rebuilds
npm run build    # release build + installer (NSIS on Windows, .app/.dmg on macOS)
```

Outputs: `src-tauri/target/release/bundle/nsis/` (Windows installer) or
`src-tauri/target/release/bundle/macos/` and `bundle/dmg/` (macOS).

The release binary is `src-tauri/target/release/mission-control(.exe)`. Settings are shared with the
Electron edition (`<config dir>/Mission Control/settings.json`), so switching editions keeps your
preferences. Run only one edition at a time: both sit in the same spot at the top of the screen.

## Where things are

| Rust module (`src-tauri/src`) | Replaces (Electron edition)                       |
| ----------------------------- | ------------------------------------------------- |
| `lib.rs`                      | `main.js`: window, tray, shortcuts, idle mode, Extreme mode, AI limits, commands |
| `bridge.js`                   | `preload.js` (plus click-through hit-testing, see below) |
| `credits.rs`                  | (new) Credits tab data: GitHub contributors and cached avatars |
| `../../rust-native/src/power.rs` | (new, shared with the pure Rust edition) Power tab readings and energy log |
| `../../rust-native/src/dock.rs`  | (new, shared with the pure Rust edition) Extreme mode dock, Windows |
| `stats.rs`                    | `src/stats.js`                                    |
| `network.rs`                  | `src/network.js`                                  |
| `cleaner.rs`                  | `src/cleaner.js`                                  |
| `processes.rs`                | `src/processes-windows.js`, `src/macos/processes.js` |
| `claude.rs`, `codex.rs`       | `src/claude.js`, `src/codex.js`                   |
| `sys_win.rs`                  | `src/syscontrol-windows.js` + `src/sysctl.ps1`    |
| `sys_mac.rs`                  | `src/macos/syscontrol.js`                         |
| `native_win.rs`               | `src/native-windows.js`, parts of `src/extreme-windows.js` |
| `native_mac.rs`               | `src/macos/native.js`, `src/macos/run.js`         |
| `extreme.rs`                  | `src/extreme-windows.js`                          |
| `login.rs`                    | launch-at-startup code in `main.js`               |
| `settings.rs`                 | `src/settings.js`                                 |

### Notes on the port

- **Click-through.** Electron forwards mouse moves to a click-through window and Tauri does not, so
  the Rust side streams the cursor position (it already did, for the pet's eyes) and `bridge.js`
  hit-tests it against the page. When the pointer reaches part of the widget, the page's own
  `mousemove` handler runs and switches click-through off, exactly as in Electron.
- **Shared with the pure Rust edition.** That edition compiles this folder's backend files; in return
  this one compiles its `power.rs` and `dock.rs` (via `#[path]`), so a fix in either lands in both.
  `power.rs` reads settings through `crate::backend::Backend`, which `lib.rs` maps to its own state.
- **Extreme mode** shows the dock instead of the Electron edition's glow line along the taskbar,
  as the pure Rust edition does. The renderer still "opens" the line's window, and `bridge.js`
  answers with a stand-in that does nothing.
- **Window visibility.** Windows are created visible but unfocused (so the widget never steals focus),
  and idle mode hides and shows them with direct Win32 calls. This keeps Tauri's own visibility flag
  in step; otherwise it re-applies "hidden" whenever click-through toggles.
- **Fixes over the Electron edition** (also worth porting back):
  - Cloudflare's `/meta` endpoint returns `{}` without the speed test's `Referer`, so the server and
    ISP were never shown. The Rust edition sends it.
  - Upload speed is counted as bytes go out, not when the server replies (which can be early and
    over-counts).
  - Ending a task is refused for protected processes on Windows too, not only on macOS.

## Testing

`MC_SELFTEST=<file.json>` makes the app drive the same `window.mc` API the interface uses (stats,
latency, processes, audio, radios, Bluetooth, brightness, Wi-Fi, power plans, limits, cache scan, power
readings, contributors, the tab layout, and no-op writes such as setting the volume to its current
value) and write what each call returned. Add `MC_SELFTEST_SPEED=1` for a speed test and
`MC_SELFTEST_CHAT=claude` for one chat turn.

Other dev switches: `MC_SETTINGS_DIR` (use a throwaway settings folder, including the power log),
`MC_NO_LOGIN=1` (don't touch the startup entry), `MC_LOGIN_TASK=<name>` (test the startup task under
another name), `MC_NO_AI=1` (preview the layout without Claude Code or Codex), `MC_DIAG_POWER=1` (log
every power reading) and `MC_POWER_ESTIMATE=1` (ignore the CPU and NVIDIA sensors). `MC_SHOTS` /
`MC_DIAG` keep their meaning of "never change system colours, the taskbar or startup".

### Status

- **Windows 11:** every feature tested on a real PC: the notch, hover and click-through, all panel
  tabs, chat (Claude Code), speed test (matches curl at the same parallelism), cache scan, audio,
  brightness, dark mode, power plans, fullscreen idle, collapse on blur, Ctrl+Alt+M, and launch at
  startup. Also tested: the Power tab (on a Ryzen 7 7700 with an RTX 5060 Ti, both measured, with
  the built-in Radeon counted in the CPU), Credits with avatars, the new charts, and Extreme mode
  with the dock. Turning it off, or quitting while it's on, restores all six colour values, the
  taskbar's auto-hide setting and the work area. Not tested: Wi-Fi and Bluetooth controls (the test
  PC has neither adapter; the Electron helper reports the same), Codex chat (not installed),
  actually emptying caches, and laptop battery readings.
- **macOS:** the Rust code type-checks for `aarch64-apple-darwin`, and the workflow in
  `.github/workflows/rust-edition.yml` builds and bundles it on a macOS runner. It has not yet been
  run on a Mac.
