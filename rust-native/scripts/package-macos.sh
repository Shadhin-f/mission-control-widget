#!/bin/bash
# Build the pure Rust edition on the current Mac; no Node.js or webview is required.
set -euo pipefail
crate_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$crate_dir"
if [[ "$(uname -s)" != Darwin ]]; then
  echo "Build the macOS installer on a Mac." >&2
  exit 1
fi
cargo build --release --locked
version="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -1)"
case "$(uname -m)" in
  arm64) arch=aarch64 ;;
  x86_64) arch=x64 ;;
  *) echo "Unsupported Mac architecture" >&2; exit 1 ;;
esac
output_dir="${1:-$crate_dir/dist}"
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"
stage="$(mktemp -d "${TMPDIR:-/tmp}/mission-control-native.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
bundle="$stage/image/Mission Control Native.app"
contents="$bundle/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"
cp target/release/mission-control "$contents/MacOS/mission-control"
cp macos/Info.plist "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string mission-control" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string com.missioncontrol.widget.native" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleName string Mission Control Native" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleDisplayName string Mission Control Native" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string APPL" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $version" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $version" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string icon.icns" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :LSMinimumSystemVersion string 12.0" "$contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :NSHighResolutionCapable bool true" "$contents/Info.plist"
/usr/bin/lipo -verify_arch "$(uname -m)" "$contents/MacOS/mission-control"
/usr/bin/lipo -verify_arch "$(uname -m)" helper/MissionControlHelper.app/Contents/MacOS/helper
cp ../rust/src-tauri/icons/icon.icns "$contents/Resources/icon.icns"
/usr/bin/ditto --norsrc --noextattr helper/MissionControlHelper.app "$contents/Resources/MissionControlHelper.app"
# Stage outside synced source folders: Finder metadata added by file providers breaks signing.
/usr/bin/xattr -cr "$bundle"
/usr/bin/codesign --force --deep --sign - "$bundle"
/usr/bin/codesign --verify --deep --strict "$bundle"
/usr/bin/plutil -lint "$contents/Info.plist"
ln -s /Applications "$stage/image/Applications"
dmg="$output_dir/Mission Control Native_${version}_${arch}.dmg"
/usr/bin/hdiutil create -volname "Mission Control Native" -srcfolder "$stage/image" -format UDZO -ov "$dmg"
/usr/bin/hdiutil verify "$dmg"
/usr/bin/shasum -a 256 "$dmg"
