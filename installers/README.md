# Installers (Rust edition)

Ready-to-install builds of the [Rust edition](../rust/README.md). You don't need Rust, Node.js or any
developer tools to use them.

| File | For | Size |
| --- | --- | --- |
| `Mission Control_1.0.0_x64-setup.exe` | Windows 10 / 11, 64-bit | 1.9 MB |
| `Mission Control_1.0.0_aarch64.dmg` | macOS 12+ on Apple Silicon (M1 and later) | 2.9 MB |
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

> The macOS build is produced by CI on GitHub's Mac runners and has not been tested by hand on a Mac yet.

## Checksums (SHA-256)

```
9f3fbb5b2c5ab6e6deaeeb2f90732dbbdaff3f9503540b0d96b417e7bb5205f8  Mission Control_1.0.0_x64-setup.exe
81ccd0f3fa5673465b9e6c2b300a8ac3adc5c783ca7e395d146d56a2c4b2a643  Mission Control Native_1.0.0_x64-setup.exe
b5681db6bf8055fe4f9fedb61f069996432a2d5b197ea7348e9d26b9588c8ae8  Mission Control_1.0.0_aarch64.dmg
```

To check a download on Windows, run `Get-FileHash <file>` in PowerShell. On a Mac, run
`shasum -a 256 <file>`.
