import Cocoa
import CoreAudio
import CoreWLAN
import CoreLocation
import IOBluetooth
import Metal
import Darwin

func addr(_ sel: AudioObjectPropertySelector, _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal, _ el: UInt32 = 0) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: sel, mScope: scope, mElement: el)
}
func get<T>(_ id: UInt32, _ a: AudioObjectPropertyAddress, _ fallback: T) -> T {
    var a = a; var value = fallback; var size = UInt32(MemoryLayout<T>.size)
    _ = withUnsafeMutablePointer(to: &value) { AudioObjectGetPropertyData(id, &a, 0, nil, &size, $0) }
    return value
}
func set<T>(_ id: UInt32, _ a: AudioObjectPropertyAddress, _ value: T) throws {
    var a = a; var value = value
    let result = withUnsafePointer(to: &value) { AudioObjectSetPropertyData(id, &a, 0, nil, UInt32(MemoryLayout<T>.size), $0) }
    if result != noErr { throw NSError(domain: "Device does not support this control (\(result)). Open System Settings.", code: Int(result)) }
}
func devices(_ input: Bool) -> [[String: Any]] {
    let scope = input ? kAudioDevicePropertyScopeInput : kAudioDevicePropertyScopeOutput
    var a = addr(kAudioHardwarePropertyDevices); var size: UInt32 = 0
    AudioObjectGetPropertyDataSize(1, &a, 0, nil, &size)
    var ids = [UInt32](repeating: 0, count: Int(size)/4)
    _ = ids.withUnsafeMutableBytes { AudioObjectGetPropertyData(1, &a, 0, nil, &size, $0.baseAddress!) }
    let def: UInt32 = get(1, addr(input ? kAudioHardwarePropertyDefaultInputDevice : kAudioHardwarePropertyDefaultOutputDevice), UInt32(0))
    return ids.compactMap { id in
        var streams = addr(kAudioDevicePropertyStreams, scope); var n: UInt32 = 0
        AudioObjectGetPropertyDataSize(id, &streams, 0, nil, &n)
        if n == 0 { return nil }
        var name: CFString = "Audio device" as CFString; var bytes = UInt32(MemoryLayout<CFString>.size)
        var na = addr(kAudioObjectPropertyName)
        _ = withUnsafeMutablePointer(to: &name) { AudioObjectGetPropertyData(id, &na, 0, nil, &bytes, $0) }
        let volume: Float32 = get(id, addr(kAudioDevicePropertyVolumeScalar, scope), Float32(-1))
        let channel: Float32 = get(id, addr(kAudioDevicePropertyVolumeScalar, scope, 1), Float32(1))
        let mute: UInt32 = get(id, addr(kAudioDevicePropertyMute, scope), UInt32(0))
        return ["id": "\(id):\(input ? "in" : "out")", "name": name as String, "isDefault": id == def, "volume": Int((volume < 0 ? channel : volume)*100), "muted": mute != 0]
    }
}
func windows() -> [[String: Any]] {
    (CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []).filter { ($0[kCGWindowLayer as String] as? Int) == 0 }
}
final class LocationDelegate: NSObject, CLLocationManagerDelegate {}
func wifi() -> [String: Any] {
    guard let i = CWWiFiClient.shared().interface() else { return ["available": false] }
    // Scanning is explicit: macOS may require Location permission to reveal SSIDs.
    let scan = CommandLine.arguments.contains("scan")
    let manager = CLLocationManager()
    let delegate = LocationDelegate()
    manager.delegate = delegate
    let allowed = manager.authorizationStatus == .authorizedAlways || manager.authorizationStatus == .authorized
    let nets = scan && allowed ? ((try? i.scanForNetworks(withSSID: nil)) ?? []) : []
    let known = Set((i.configuration()?.networkProfiles.array as? [CWNetworkProfile] ?? []).compactMap { $0.ssid })
    return ["available": true, "powered": i.powerOn(), "interface": i.interfaceName ?? "en0", "current": ["state": i.ssid() != nil || (i.powerOn() && i.rssiValue() < 0) ? "connected" : "disconnected", "ssid": i.ssid() ?? "SSID hidden by macOS", "signal": max(0, min(100, (i.rssiValue()+100)*2))], "networks": nets.compactMap { n -> [String: Any]? in guard let ssid = n.ssid else { return nil }; return ["ssid": ssid, "signal": max(0,min(100,(n.rssiValue+100)*2)), "secure": !n.supportsSecurity(.none), "known": known.contains(ssid)] }, "needsLocation": i.powerOn() && i.ssid() == nil]
}
func brightness(_ value: Float? = nil, _ index: UInt32 = 0) throws -> [[String: Any]] {
    // DisplayServices covers built-in Apple displays; unsupported external displays are omitted.
    guard let lib = dlopen("/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices", RTLD_LAZY), let symbol = dlsym(lib, "DisplayServicesGetBrightness") else { return [] }
    defer { dlclose(lib) }
    let read = unsafeBitCast(symbol, to: (@convention(c) (UInt32, UnsafeMutablePointer<Float>) -> Int32).self)
    if let value = value {
        guard let s = dlsym(lib, "DisplayServicesSetBrightness") else { throw NSError(domain: "Brightness unavailable", code: 1) }
        let write = unsafeBitCast(s, to: (@convention(c) (UInt32, Float) -> Int32).self)
        if write(index, max(0,min(1,value))) != 0 { throw NSError(domain: "Display does not support brightness control", code: 1) }
    }
    var ids = [UInt32](repeating: 0, count: 16); var count: UInt32 = 0
    CGGetActiveDisplayList(16, &ids, &count)
    return ids.prefix(Int(count)).compactMap { id in
        var v: Float = 0
        guard read(id, &v) == 0 else { return nil }
        return ["index": id, "name": "Display \(id)", "min": 0, "max": 100, "cur": Int(v*100)]
    }
}
func execute(_ cmd: String, _ a: [String: Any]) throws -> Any {
    switch cmd {
    case "state":
        let bt = IOBluetoothHostController.default()
        return ["audio": ["out": devices(false), "in": devices(true)], "radios": [["kind": "WiFi", "state": CWWiFiClient.shared().interface()?.powerOn() == true ? "On" : "Off"], ["kind": "Bluetooth", "state": bt?.powerState == kBluetoothHCIPowerStateON ? "On" : "Off"]], "dark": UserDefaults.standard.string(forKey: "AppleInterfaceStyle") == "Dark"]
    case "windows":
        let front = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
        let list = windows()
        let full = list.contains { w in
            guard (w[kCGWindowOwnerPID as String] as? Int ?? -1) == Int(front), let b = w[kCGWindowBounds as String] as? NSDictionary, let rect = CGRect(dictionaryRepresentation: b) else { return false }
            return NSScreen.screens.contains { s in let r = CGDisplayBounds(s.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as! UInt32); return abs(rect.minX-r.minX)<2 && abs(rect.minY-r.minY)<2 && abs(rect.width-r.width)<2 && abs(rect.height-r.height)<2 }
        }
        return ["fullscreen": full, "apps": list.map { ["pid": $0[kCGWindowOwnerPID as String] ?? 0, "name": $0[kCGWindowOwnerName as String] ?? "App"] }]
    case "requestLocation":
        let manager = CLLocationManager(); let delegate = LocationDelegate(); manager.delegate = delegate
        manager.requestWhenInUseAuthorization()
        let deadline = Date().addingTimeInterval(10)
        while manager.authorizationStatus == .notDetermined && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.1)) }
        return ["ok": manager.authorizationStatus == .authorizedAlways || manager.authorizationStatus == .authorized]
    case "wifi": return wifi()
    case "disconnectWifi": CWWiFiClient.shared().interface()?.disassociate(); return ["ok": true]
    case "setWifi": try CWWiFiClient.shared().interface()?.setPower(a["on"] as? Bool ?? false); return ["ok": true]
    case "bt": return ["devices": (IOBluetoothDevice.pairedDevices() as? [IOBluetoothDevice] ?? []).map { ["name": $0.name ?? "Bluetooth device", "connected": $0.isConnected(), "kind": "Peripheral"] }]
    case "brightness": return ["monitors": try brightness()]
    case "setBrightness": _ = try brightness(Float(a["value"] as? Double ?? 50)/100, UInt32(a["index"] as? Int ?? 0)); return ["ok": true]
    case "media":
        let key = ["toggle": 16, "next": 17, "prev": 18][a["action"] as? String ?? ""]
        guard let key = key else { throw NSError(domain: "Invalid media action", code: 1) }
        for flag in [0xA, 0xB] {
            let event = NSEvent.otherEvent(with: .systemDefined, location: .zero, modifierFlags: NSEvent.ModifierFlags(rawValue: UInt(flag << 8)), timestamp: 0, windowNumber: 0, context: nil, subtype: 8, data1: (key << 16) | (flag << 8), data2: -1)
            event?.cgEvent?.post(tap: .cghidEventTap)
        }
        return ["ok": true]
    case "gpu": return ["name": MTLCreateSystemDefaultDevice()?.name ?? "GPU", "unified": MTLCreateSystemDefaultDevice()?.hasUnifiedMemory ?? false]
    case "setVolume", "setMute", "setDefault":
        let parts = (a["id"] as? String ?? "").split(separator: ":")
        guard parts.count == 2, let id = UInt32(parts[0]) else { throw NSError(domain: "Invalid audio device", code: 1) }
        let input = parts[1] == "in", scope = input ? kAudioDevicePropertyScopeInput : kAudioDevicePropertyScopeOutput
        if cmd == "setDefault" { try set(1, addr(input ? kAudioHardwarePropertyDefaultInputDevice : kAudioHardwarePropertyDefaultOutputDevice), id) }
        else if cmd == "setMute" { try set(id, addr(kAudioDevicePropertyMute, scope), UInt32(a["muted"] as? Bool == true ? 1 : 0)) }
        else {
            let v = Float32(max(0,min(100,a["value"] as? Double ?? 50))/100)
            if (try? set(id, addr(kAudioDevicePropertyVolumeScalar, scope), v)) == nil {
                try set(id, addr(kAudioDevicePropertyVolumeScalar, scope, 1), v)
                try? set(id, addr(kAudioDevicePropertyVolumeScalar, scope, 2), v)
            }
            try? set(id, addr(kAudioDevicePropertyMute, scope), UInt32(0))
        }
        return ["ok": true]
    default: throw NSError(domain: "Unknown command", code: 1)
    }
}
do {
    let cmd = CommandLine.arguments.dropFirst().first ?? "state"
    let data = (CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "{}").data(using: .utf8)!
    let args = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    let result = try execute(cmd,args)
    let out = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
    print(String(data: out, encoding: .utf8)!)
} catch { fputs("\(error.localizedDescription)\n", stderr); exit(1) }
