# macOS support

This fork supports macOS on Apple Silicon and Intel, alongside the original Windows backend.

## Development

Install Node.js 18+ and Xcode Command Line Tools (`xcode-select --install`). Run `npm install`, then `npm start`. The postinstall script compiles the Swift helper for the current Mac architecture. For Bluetooth power control, install `blueutil` with `brew install blueutil`.

`npm test` exercises native read APIs and cleaner safety without changing system settings. `MC_SHOTS=/absolute/existing/folder npm start` captures UI previews without enabling startup. No test shuts down, sleeps, kills other apps, changes radios or deletes actual caches.

## Feature behavior

| Feature | macOS implementation |
| --- | --- |
| Pet, panel, chat, tray | Electron; top of the usable display area below the menu bar/notch; visible across desktops |
| CPU, memory, disks | Node OS APIs, vm_stat and filesystem statistics; RAM includes reclaimable inactive pages as available |
| GPU | Metal device identification and IOAccelerator utilization where exposed; Apple Silicon uses unified RAM rather than dedicated VRAM |
| Network, ping, speed test | Asynchronous netstat and native ping; existing TCP download/upload test and history |
| Fullscreen idle | Core Graphics frontmost window/display bounds; window names may require Screen Recording permission |
| Audio | Core Audio output/input enumeration, switching, volume and hardware mute where supported by the device |
| Brightness | Apple DisplayServices; supported built-in/Apple displays; other monitors use their physical controls/System Settings |
| Wi-Fi | CoreWLAN radio power, status, scanning and disconnect; networksetup connects saved networks. macOS can redact SSIDs/scans without Location permission; System Settings remains available |
| Bluetooth | IOBluetooth paired/connected device status; blueutil radio power; pairing/connecting through System Settings |
| Theme | AppleScript appearance preference; may require Automation permission |
| Media | Music/Spotify metadata and control through AppleScript; media keys for other players (may require Accessibility permission). There is no public universal macOS now-playing metadata API |
| Energy modes | Automatic/Low Power via pmset with the macOS administrator prompt on explicit user action |
| Lock, sleep, restart, shutdown | Native session commands; existing UI confirmation for sleep/restart/shutdown retained |
| Processes | ps resident memory, Core Graphics app identification, SIGTERM. System/other-user/widget processes are protected again at the backend |
| Cleaner | User temp, Chrome/Edge/Brave/Firefox/Safari caches, Metal cache, user crash reports, npm/pip and user Trash. Windows-only update/thumbnail caches are omitted. Full Disk Access may be needed for Safari/Trash. Symlinks are not followed and recent nested temp files are retained |
| Startup | Packaged app uses Electron login-item registration; source runs write a user LaunchAgent for next login. Moving the source requires launching it again |
| Claude/Codex | Executable PATH/Homebrew/local-bin discovery, macOS prompts, Unix process-group cancellation; existing chat, session resumption and rate-limit bridges retained |

## Platform limits

GPU temperature, GPU power and fan readings are unavailable when macOS does not expose them without privileges; the UI does not invent numbers. Some devices do not expose hardware volume/mute or brightness. An unsigned/local app may need permission in System Settings → Privacy & Security (Automation, Accessibility, Screen Recording, Full Disk Access or Location). Denied access is surfaced as an error or unavailable data. No permissions are bypassed.

The DisplayServices brightness API and IOAccelerator performance fields are Apple implementation details and may change between macOS releases. This local build is ad-hoc signed, not notarized for public distribution. Windows code is preserved in `src/*-windows.js`; Windows execution still needs validation on a Windows machine.
