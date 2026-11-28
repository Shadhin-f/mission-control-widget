# Installers (Rust edition)

Ready-to-install builds of the [Rust edition](../rust/README.md). You don't need Rust, Node.js or any
developer tools to use them.

| File | For | Size |
| --- | --- | --- |
| `Mission Control_1.0.0_x64-setup.exe` | Windows 10 / 11, 64-bit | 1.9 MB |
| `Mission Control_1.0.0_aarch64.dmg` | macOS 12+ on Apple Silicon (M1 and later) | 2.9 MB |
| `Mission Control Native_1.0.0_aarch64.dmg` | macOS 12+ on Apple Silicon: the **pure Rust edition**, built and smoke-tested locally on an Apple M4 Mac | 3.5 MB |
| `Mission Control Native_1.0.0_x64-setup.exe` | Windows 10 / 11, 64-bit: the [pure Rust edition](../rust-native/README.md) (no web view) | 4.1 MB |

There is no Intel Mac build yet. On an Intel Mac, build from source (see the main README).

## Windows

1. Download `Mission Control_1.0.0_x64-setup.exe` and run it. It installs for your user account
   only, so no admin rights are needed.
2. Windows SmartScreen may say it "protected your PC", because the installer isn't code-signed.
   Click **More info**, then **Run anyway**.
3. Start Mission Control from the installer's last page or from the Start menu. The widget appears
   at the top of your screen.

The widget needs Microsoft's **WebView2** runtime. Windows 11 always has it; if a Windows 10 PC
doesn't, the installer downloads it during setup.

## macOS (Apple Silicon)

1. Download `Mission Control_1.0.0_aarch64.dmg`, open it and drag **Mission Control** into
   **Applications**.
2. The app isn't notarized by Apple. The first time, Control-click it in Applications and choose
   **Open**, or open it and then click **Open Anyway** in
   **System Settings → Privacy & Security**.
3. Allow the permissions you want to use when macOS asks. The table in the main README's
   [macOS quick start](../README.md#macos-quick-start) explains each one.

### Pure Rust edition (Apple Silicon)

For the native egui app with no webview, download **`Mission Control Native_1.0.0_aarch64.dmg`** instead. Open the image and drag **Mission Control Native** to Applications. Its bundle identifier is `com.missioncontrol.widget.native`; it is distinct from the Tauri app above. The local build is ad-hoc signed, not notarized.

This installer was compiled from `rust-native` and tested on an Apple M4 running macOS 27.0.1. Checks covered bundle/helper signatures, six read-only native queries, all ten UI screens and normal exit with isolated settings and startup disabled. Older macOS versions, Intel hardware, destructive controls and AI message sending were not run-tested. Reproduce the build and smoke checks with the [native edition instructions](../rust-native/README.md#build-from-source).

The older Tauri macOS installer above was produced by CI and has not been hand-tested here.

## Checksums (SHA-256)

```
2993652b9a9f8f1ad3aa6dc84502766017953709b2ba56e4980b4fe8a1d7349e  Mission Control Native_1.0.0_aarch64.dmg
9f3fbb5b2c5ab6e6deaeeb2f90732dbbdaff3f9503540b0d96b417e7bb5205f8  Mission Control_1.0.0_x64-setup.exe
451a738b7a05e22860b8044080309880d558197482fc236cd0766cf682036d2c  Mission Control Native_1.0.0_x64-setup.exe
b5681db6bf8055fe4f9fedb61f069996432a2d5b197ea7348e9d26b9588c8ae8  Mission Control_1.0.0_aarch64.dmg
```

To check a download on Windows, run `Get-FileHash <file>` in PowerShell. On a Mac, run
`shasum -a 256 <file>`.
