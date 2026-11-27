# Mission Control: Rust edition

A second build of Mission Control with the backend rewritten in Rust on [Tauri 2](https://tauri.app).
It looks and behaves like the Electron edition because it **runs the same interface**: `../renderer`
is used unchanged. A small bridge script (`src-tauri/src/bridge.js`) gives that interface the same
`window.mc` API that Electron's `preload.js` provides, and every call behind it is Rust.

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
| `lib.rs`                      | `main.js`: window, tray, shortcuts, idle mode, Extreme glow line, AI limits, commands |
| `bridge.js`                   | `preload.js` (plus click-through hit-testing, see below) |
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
- **Extreme mode's glow line** is a second, click-through window. The renderer still "opens" it
  with `window.open` and writes its HTML; `bridge.js` forwards that to Rust, which serves the page
  and places the window on the taskbar's top edge.
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
latency, processes, audio, radios, Bluetooth, brightness, Wi-Fi, power plans, limits, cache scan, and
no-op writes such as setting the volume to its current value) and write what each call returned.
Add `MC_SELFTEST_SPEED=1` for a speed test and `MC_SELFTEST_CHAT=claude` for one chat turn.

Other dev switches: `MC_SETTINGS_DIR` (use a throwaway settings folder), `MC_NO_LOGIN=1` (don't
touch the startup entry), `MC_LOGIN_TASK=<name>` (test the startup task under another name),
`MC_NO_AI=1` (preview the layout without Claude Code or Codex), `MC_DIAG_EDGE=1` (log glow-line
placement). `MC_SHOTS` / `MC_DIAG` keep the Electron meaning of "never change system colours or startup".

### Status

- **Windows 11:** every feature tested on a real PC: the notch, hover and click-through, all panel
  tabs, chat (Claude Code), speed test (matches curl at the same parallelism), cache scan, audio,
  brightness, dark mode, power plans, Extreme mode (themes the taskbar, glow line, and restores all
  six colour values on toggle-off and on quit), fullscreen idle, collapse on blur, Ctrl+Alt+M, and
  launch at startup. Not tested: Wi-Fi and Bluetooth controls (the test PC has neither adapter;
  the Electron helper reports the same), Codex chat (not installed), and actually emptying caches.
- **macOS:** the Rust code type-checks for `aarch64-apple-darwin`, and the workflow in
  `.github/workflows/rust-edition.yml` builds and bundles it on a macOS runner. It has not yet been
  run on a Mac.
