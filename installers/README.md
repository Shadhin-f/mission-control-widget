# Installers (Rust edition)

Ready-to-install builds of the [Rust edition](../rust/README.md). You don't need Rust, Node.js or any
developer tools to use them.

| File | For | Size |
| --- | --- | --- |
| `Mission Control_1.1.0_x64-setup.exe` | Windows 10 / 11, 64-bit: the Tauri Rust edition, with the Power and Credits tabs and the Extreme mode dock | 2.1 MB |
| `Mission Control_1.1.0_aarch64.dmg` | macOS 12+ on Apple Silicon (M1 and later): the Tauri Rust edition | 2.9 MB |
| `Mission Control Native_1.0.0_aarch64.dmg` | macOS 12+ on Apple Silicon: the **pure Rust edition**, built and smoke-tested locally on an Apple M4 Mac | 3.5 MB |
| `Mission Control Native_1.0.0_x64-setup.exe` | Windows 10 / 11, 64-bit: the [pure Rust edition](../rust-native/README.md) (no web view) | 4.1 MB |

There is no Intel Mac build yet. On an Intel Mac, build from source (see the main README).

## Windows

1. Download `Mission Control_1.1.0_x64-setup.exe` and run it. It installs for your user account
   only, so no admin rights are needed. Over an earlier version it updates in place, keeping your
   settings.
2. Windows SmartScreen may say it "protected your PC", because the installer isn't code-signed.
   Click **More info**, then **Run anyway**.
3. Start Mission Control from the installer's last page or from the Start menu. The widget appears
   at the top of your screen.

The widget needs Microsoft's **WebView2** runtime. Windows 11 always has it; if a Windows 10 PC
doesn't, the installer downloads it during setup.

## macOS (Apple Silicon)

1. Download `Mission Control_1.1.0_aarch64.dmg`, open it and drag **Mission Control** into
   **Applications** (replacing an older copy if you have one).
2. The app isn't notarized by Apple. The first time, Control-click it in Applications and choose
   **Open**, or open it and then click **Open Anyway** in
   **System Settings → Privacy & Security**.
3. Allow the permissions you want to use when macOS asks. The table in the main README's
   [macOS quick start](../README.md#macos-quick-start) explains each one.

### Pure Rust edition (Apple Silicon)

For the native egui app with no webview, download **`Mission Control Native_1.0.0_aarch64.dmg`** instead. Open the image and drag **Mission Control Native** to Applications. Its bundle identifier is `com.missioncontrol.widget.native`; it is distinct from the Tauri app above. The local build is ad-hoc signed, not notarized.

This installer was compiled from `rust-native` and tested on an Apple M4 running macOS 27.0.1. The Mac build includes the Power tab with battery/adapter telemetry, cost projections, history and calibration; component and wall-power estimates are labelled. Six regression tests passed. Checks covered bundle/helper signatures, seven read-only native queries, all eleven UI screens (including Power) and normal exit with isolated settings and startup disabled. Older macOS versions, Intel hardware, destructive controls and AI message sending were not run-tested. Reproduce the build and smoke checks with the [native edition instructions](../rust-native/README.md#build-from-source).

The Tauri macOS installer above was produced by CI (the `Rust edition` workflow on GitHub's macOS runner) and has not been hand-tested on a Mac. The Tauri Windows installer was built and tested on Windows 11.

## Checksums (SHA-256)

```
8d1234607d980d8ef31ce0bc0ed8d315cfa88000869335579697876f0c48a8f9  Mission Control Native_1.0.0_aarch64.dmg
1c6409394a66bd5cc39ac74b2a25f60949649e05b78811376d21c4c4ed562c4f  Mission Control_1.1.0_x64-setup.exe
c272667a5b57ac7c540df4cefb3a6c18485b1e9a04e361e97e36bf439c88bf8d  Mission Control Native_1.0.0_x64-setup.exe
5e58b20320f4a3fa7f22baa1b055b75516296b98a45026aa22abb8176744a6c7  Mission Control_1.1.0_aarch64.dmg
```

To check a download on Windows, run `Get-FileHash <file>` in PowerShell. On a Mac, run
`shasum -a 256 <file>`.
