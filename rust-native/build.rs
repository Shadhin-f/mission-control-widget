// macOS targets: builds the small Swift helper that reads CoreAudio, CoreWLAN, Bluetooth and window
// data (the same helper the other editions use, from ../src/macos). The packager puts it in the app's
// Resources folder. Other targets need nothing.
fn main() {
    // Decide by the *target* (build scripts are compiled for the host machine)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_helper();
    }
}

fn build_macos_helper() {
    use std::path::PathBuf;
    use std::process::Command;
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let swift = manifest.join("../src/macos/Helper.swift");
    println!("cargo:rerun-if-changed={}", swift.display());
    // File-provider/Finder metadata in synced workspaces can invalidate code signing. Build and sign
    // in a temporary directory, then copy the resource into the crate for packaging/dev runs.
    let stage = std::env::temp_dir().join(format!("mission-control-helper-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let bundle = stage.join("MissionControlHelper.app");
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::write(contents.join("Info.plist"), r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.missioncontrol.helper</string><key>CFBundleName</key><string>Mission Control Helper</string><key>CFBundleExecutable</key><string>helper</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/><key>NSLocationUsageDescription</key><string>Show nearby Wi-Fi networks and the current network name.</string><key>NSLocationWhenInUseUsageDescription</key><string>Show nearby Wi-Fi networks and the current network name.</string></dict></plist>"#).unwrap();
    if !cfg!(target_os = "macos") {
        // Cross-checking from Windows/Linux: swiftc isn't available, and the bundle is only needed on a Mac
        println!("cargo:warning=Not on macOS: skipping the Swift helper build (needed only when running on a Mac)");
        return;
    }
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        _ => panic!("Unsupported macOS target architecture"),
    };
    let swift_target = format!("{arch}-apple-macosx12.0");
    let status = Command::new("/usr/bin/xcrun")
        .args(["swiftc", "-O", "-target", &swift_target])
        .arg(&swift)
        .arg("-o")
        .arg(contents.join("MacOS/helper"))
        .status()
        .expect("xcrun swiftc is required to build the macOS helper (install the Xcode command line tools)");
    assert!(status.success(), "swiftc failed to build the macOS helper");
    let _ = Command::new("/usr/bin/xattr").arg("-cr").arg(&bundle).status();
    let signed = Command::new("/usr/bin/codesign").args(["--force", "--sign", "-", "--identifier", "com.missioncontrol.helper"]).arg(&bundle).status().expect("codesign is required");
    assert!(signed.success(), "Failed to sign the macOS helper");
    let destination = manifest.join("helper/MissionControlHelper.app");
    let _ = std::fs::remove_dir_all(&destination);
    let copied = Command::new("/usr/bin/ditto").args(["--norsrc", "--noextattr"]).arg(&bundle).arg(&destination).status().expect("ditto is required");
    assert!(copied.success(), "Failed to copy the macOS helper");
    let _ = std::fs::remove_dir_all(stage);
}
