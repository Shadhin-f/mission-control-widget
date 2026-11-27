// Builds the Tauri context and, for macOS targets, the small Swift helper that reads CoreAudio, CoreWLAN,
// Bluetooth and window data (the same helper the Electron edition uses, from ../../src/macos).
fn main() {
    // Decide by the *target* (build scripts are compiled for the host machine)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_helper();
    }
    tauri_build::build()
}

fn build_macos_helper() {
    use std::path::PathBuf;
    use std::process::Command;
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let swift = manifest.join("../../src/macos/Helper.swift");
    println!("cargo:rerun-if-changed={}", swift.display());
    let bundle = manifest.join("helper/MissionControlHelper.app");
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::write(contents.join("Info.plist"), r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.missioncontrol.helper</string><key>CFBundleName</key><string>Mission Control Helper</string><key>CFBundleExecutable</key><string>helper</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/><key>NSLocationUsageDescription</key><string>Show nearby Wi-Fi networks and the current network name.</string><key>NSLocationWhenInUseUsageDescription</key><string>Show nearby Wi-Fi networks and the current network name.</string></dict></plist>"#).unwrap();
    if !cfg!(target_os = "macos") {
        // Cross-checking from Windows/Linux: swiftc isn't available, and the bundle is only needed on a Mac
        println!("cargo:warning=Not on macOS: skipping the Swift helper build (needed only when running on a Mac)");
        return;
    }
    let status = Command::new("/usr/bin/xcrun")
        .args(["swiftc", "-O"])
        .arg(&swift)
        .arg("-o")
        .arg(contents.join("MacOS/helper"))
        .status()
        .expect("xcrun swiftc is required to build the macOS helper (install the Xcode command line tools)");
    assert!(status.success(), "swiftc failed to build the macOS helper");
    let _ = Command::new("/usr/bin/xattr").arg("-cr").arg(&bundle).status();
    let _ = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--identifier", "com.missioncontrol.helper"])
        .arg(&bundle)
        .status();
}
