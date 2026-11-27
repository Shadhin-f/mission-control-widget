# Mission Control system helper.
# A long-lived process: reads one JSON request per line on stdin, writes one JSON reply per line on stdout.
# Talks to Windows directly: Core Audio (devices, volume, mute, default device), radios (Wi-Fi/Bluetooth),
# paired Bluetooth devices, the system media session, DDC/CI monitor brightness, dark mode and power.

$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::InputEncoding = $utf8
[Console]::OutputEncoding = $utf8

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace MC {
  [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumeratorCo {}
  [ComImport, Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")] class PolicyConfigClient {}

  [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDeviceEnumerator {
    int EnumAudioEndpoints(int dataFlow, int stateMask, out IMMDeviceCollection devices);
    int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice endpoint);
    int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out IMMDevice device);
  }
  [ComImport, Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDeviceCollection {
    int GetCount(out int count);
    int Item(int index, out IMMDevice device);
  }
  [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDevice {
    int Activate(ref Guid iid, int clsCtx, IntPtr activationParams, [MarshalAs(UnmanagedType.IUnknown)] out object iface);
    int OpenPropertyStore(int access, out IPropertyStore store);
    int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
    int GetState(out int state);
  }
  [ComImport, Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IPropertyStore {
    int GetCount(out int count);
    int GetAt(int prop, out PROPERTYKEY key);
    int GetValue(ref PROPERTYKEY key, out PROPVARIANT value);
  }
  [StructLayout(LayoutKind.Sequential)] struct PROPERTYKEY { public Guid fmtid; public int pid; }
  [StructLayout(LayoutKind.Explicit)] struct PROPVARIANT {
    [FieldOffset(0)] public short vt;
    [FieldOffset(8)] public IntPtr p;
    [FieldOffset(16)] public IntPtr p2;
  }
  [ComImport, Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioEndpointVolume {
    int RegisterControlChangeNotify(IntPtr n);
    int UnregisterControlChangeNotify(IntPtr n);
    int GetChannelCount(out uint c);
    int SetMasterVolumeLevel(float db, ref Guid ctx);
    int SetMasterVolumeLevelScalar(float level, ref Guid ctx);
    int GetMasterVolumeLevel(out float db);
    int GetMasterVolumeLevelScalar(out float level);
    int SetChannelVolumeLevel(uint ch, float db, ref Guid ctx);
    int SetChannelVolumeLevelScalar(uint ch, float level, ref Guid ctx);
    int GetChannelVolumeLevel(uint ch, out float db);
    int GetChannelVolumeLevelScalar(uint ch, out float level);
    int SetMute([MarshalAs(UnmanagedType.Bool)] bool mute, ref Guid ctx);
    int GetMute([MarshalAs(UnmanagedType.Bool)] out bool mute);
  }
  // Undocumented but stable since Windows 7: the only way to change the default audio device.
  [ComImport, Guid("f8679f50-850a-41cf-9c72-430f290290c8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IPolicyConfig {
    int GetMixFormat(IntPtr a, IntPtr b);
    int GetDeviceFormat(IntPtr a, int b, IntPtr c);
    int ResetDeviceFormat(IntPtr a);
    int SetDeviceFormat(IntPtr a, IntPtr b, IntPtr c);
    int GetProcessingPeriod(IntPtr a, int b, IntPtr c, IntPtr d);
    int SetProcessingPeriod(IntPtr a, IntPtr b);
    int GetShareMode(IntPtr a, IntPtr b);
    int SetShareMode(IntPtr a, IntPtr b);
    int GetPropertyValue(IntPtr a, IntPtr b, IntPtr c);
    int SetPropertyValue(IntPtr a, IntPtr b, IntPtr c);
    int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string id, int role);
    int SetEndpointVisibility(IntPtr a, int b);
  }

  public class AudioDev {
    public string Id { get; set; }
    public string Name { get; set; }
    public bool IsDefault { get; set; }
    public int Volume { get; set; }
    public bool Muted { get; set; }
  }

  public static class Audio {
    [DllImport("ole32.dll")] static extern int PropVariantClear(ref PROPVARIANT pv);

    static IMMDeviceEnumerator Enumr() { return (IMMDeviceEnumerator)(new MMDeviceEnumeratorCo()); }

    public static string DefaultId(int flow) {
      IMMDevice d;
      if (Enumr().GetDefaultAudioEndpoint(flow, 0, out d) != 0 || d == null) return null;
      string id; d.GetId(out id); return id;
    }
    static IAudioEndpointVolume Vol(IMMDevice d) {
      Guid iid = typeof(IAudioEndpointVolume).GUID; object o;
      if (d == null || d.Activate(ref iid, 23, IntPtr.Zero, out o) != 0) return null;
      return o as IAudioEndpointVolume;
    }
    static string Name(IMMDevice d) {
      IPropertyStore ps;
      if (d.OpenPropertyStore(0, out ps) != 0) return "Audio device";
      PROPERTYKEY key = new PROPERTYKEY(); key.fmtid = new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"); key.pid = 14;
      PROPVARIANT pv;
      if (ps.GetValue(ref key, out pv) != 0) return "Audio device";
      string s = pv.vt == 31 ? Marshal.PtrToStringUni(pv.p) : "Audio device";
      PropVariantClear(ref pv);
      return s;
    }
    static IMMDevice Get(string id) {
      IMMDevice d;
      if (string.IsNullOrEmpty(id)) { Enumr().GetDefaultAudioEndpoint(0, 0, out d); return d; }
      Enumr().GetDevice(id, out d); return d;
    }
    public static AudioDev[] List(int flow) {
      IMMDeviceCollection col; List<AudioDev> list = new List<AudioDev>();
      if (Enumr().EnumAudioEndpoints(flow, 1, out col) != 0) return list.ToArray();
      int n; col.GetCount(out n);
      string def = DefaultId(flow);
      for (int i = 0; i < n; i++) {
        IMMDevice d; col.Item(i, out d);
        string id; d.GetId(out id);
        AudioDev dev = new AudioDev(); dev.Id = id; dev.Name = Name(d); dev.IsDefault = id == def;
        IAudioEndpointVolume v = Vol(d);
        if (v != null) {
          float f; v.GetMasterVolumeLevelScalar(out f); dev.Volume = (int)Math.Round(f * 100);
          bool m; v.GetMute(out m); dev.Muted = m;
        }
        list.Add(dev);
      }
      return list.ToArray();
    }
    public static int SetVolume(string id, int pct) {
      IAudioEndpointVolume v = Vol(Get(id)); Guid g = Guid.Empty;
      pct = Math.Max(0, Math.Min(100, pct));
      v.SetMasterVolumeLevelScalar(pct / 100f, ref g);
      if (pct > 0) v.SetMute(false, ref g);
      return pct;
    }
    public static int StepVolume(int delta) {
      IAudioEndpointVolume v = Vol(Get(null)); float f; v.GetMasterVolumeLevelScalar(out f);
      return SetVolume(null, (int)Math.Round(f * 100) + delta);
    }
    public static bool SetMute(string id, bool mute) {
      IAudioEndpointVolume v = Vol(Get(id)); Guid g = Guid.Empty;
      v.SetMute(mute, ref g); return mute;
    }
    public static bool SetDefault(string id) {
      IPolicyConfig pc = (IPolicyConfig)(new PolicyConfigClient());
      for (int role = 0; role < 3; role++) pc.SetDefaultEndpoint(id, role);
      return true;
    }
  }

  public class MonitorInfo {
    public int Index { get; set; }
    public string Name { get; set; }
    public int Min { get; set; }
    public int Cur { get; set; }
    public int Max { get; set; }
  }

  // External monitors over DDC/CI (the same channel as the monitor's own buttons)
  public static class Display {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct PHYSICAL_MONITOR { public IntPtr h; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string desc; }
    delegate bool MonitorEnumProc(IntPtr hMon, IntPtr hdc, IntPtr rect, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc cb, IntPtr data);
    [DllImport("dxva2.dll")] static extern bool GetNumberOfPhysicalMonitorsFromHMONITOR(IntPtr hMon, out uint count);
    [DllImport("dxva2.dll")] static extern bool GetPhysicalMonitorsFromHMONITOR(IntPtr hMon, uint size, [Out] PHYSICAL_MONITOR[] arr);
    [DllImport("dxva2.dll")] static extern bool GetMonitorBrightness(IntPtr h, out uint min, out uint cur, out uint max);
    [DllImport("dxva2.dll")] static extern bool SetMonitorBrightness(IntPtr h, uint v);
    [DllImport("dxva2.dll")] static extern bool DestroyPhysicalMonitor(IntPtr h);

    static List<IntPtr> handles = new List<IntPtr>();
    static List<string> names = new List<string>();

    static void Release() { foreach (IntPtr h in handles) DestroyPhysicalMonitor(h); handles.Clear(); names.Clear(); }

    public static MonitorInfo[] List() {
      Release();
      List<IntPtr> hmons = new List<IntPtr>();
      EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, delegate (IntPtr h, IntPtr dc, IntPtr r, IntPtr d) { hmons.Add(h); return true; }, IntPtr.Zero);
      foreach (IntPtr hm in hmons) {
        uint n;
        if (!GetNumberOfPhysicalMonitorsFromHMONITOR(hm, out n) || n == 0) continue;
        PHYSICAL_MONITOR[] arr = new PHYSICAL_MONITOR[n];
        if (!GetPhysicalMonitorsFromHMONITOR(hm, n, arr)) continue;
        foreach (PHYSICAL_MONITOR pm in arr) { handles.Add(pm.h); names.Add(pm.desc); }
      }
      List<MonitorInfo> res = new List<MonitorInfo>();
      for (int i = 0; i < handles.Count; i++) {
        uint mn, cur, mx;
        if (GetMonitorBrightness(handles[i], out mn, out cur, out mx)) {
          MonitorInfo m = new MonitorInfo(); m.Index = i; m.Name = names[i]; m.Min = (int)mn; m.Cur = (int)cur; m.Max = (int)mx;
          res.Add(m);
        }
      }
      return res.ToArray();
    }
    public static bool Set(int index, int value) {
      if (index < 0 || index >= handles.Count) List();
      if (index < 0 || index >= handles.Count) return false;
      return SetMonitorBrightness(handles[index], (uint)value);
    }
  }

  public static class Sys {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll")] public static extern bool LockWorkStation();
    [DllImport("powrprof.dll")] public static extern bool SetSuspendState(bool hibernate, bool force, bool disableWake);
    // Tell running apps the light/dark theme changed
    public static void BroadcastTheme() {
      UIntPtr r;
      SendMessageTimeout((IntPtr)0xffff, 0x1A, UIntPtr.Zero, "ImmersiveColorSet", 2, 300, out r);
    }
  }
}
'@

# ---- WinRT plumbing (radios, Bluetooth, media session) ----
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$script:asTaskOp = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
  $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
} | Select-Object -First 1
function Await($op, [Type]$type) {
  $t = $script:asTaskOp.MakeGenericMethod($type).Invoke($null, @($op))
  if (-not $t.Wait(8000)) { throw 'Windows did not answer in time' }
  return $t.Result
}
$null = [Windows.Devices.Radios.Radio, Windows.System.Devices, ContentType = WindowsRuntime]
$null = [Windows.Devices.Enumeration.DeviceInformation, Windows.Devices.Enumeration, ContentType = WindowsRuntime]
$null = [Windows.Devices.Bluetooth.BluetoothDevice, Windows.Devices.Bluetooth, ContentType = WindowsRuntime]
$null = [Windows.Devices.Bluetooth.BluetoothLEDevice, Windows.Devices.Bluetooth, ContentType = WindowsRuntime]
$null = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime]
$RadioList = [System.Collections.Generic.IReadOnlyList[Windows.Devices.Radios.Radio]]

function Get-Radios {
  $list = Await ([Windows.Devices.Radios.Radio]::GetRadiosAsync()) $RadioList
  return @($list | ForEach-Object { @{ kind = "$($_.Kind)"; state = "$($_.State)"; name = $_.Name } })
}
function Set-Radio([string]$kind, [bool]$on) {
  $access = Await ([Windows.Devices.Radios.Radio]::RequestAccessAsync()) ([Windows.Devices.Radios.RadioAccessStatus])
  if ("$access" -ne 'Allowed') { throw "Windows denied radio access ($access)" }
  $list = Await ([Windows.Devices.Radios.Radio]::GetRadiosAsync()) $RadioList
  $r = $list | Where-Object { "$($_.Kind)" -eq $kind } | Select-Object -First 1
  if (-not $r) { throw "No $kind adapter found" }
  $state = if ($on) { [Windows.Devices.Radios.RadioState]::On } else { [Windows.Devices.Radios.RadioState]::Off }
  $res = Await ($r.SetStateAsync($state)) ([Windows.Devices.Radios.RadioAccessStatus])
  if ("$res" -ne 'Allowed') { throw "Could not switch $kind ($res)" }
  return $on
}

function Get-BtDevices {
  $out = @(); $seen = @{}
  $sources = @(
    @{ sel = [Windows.Devices.Bluetooth.BluetoothDevice]::GetDeviceSelectorFromPairingState($true); le = $false },
    @{ sel = [Windows.Devices.Bluetooth.BluetoothLEDevice]::GetDeviceSelectorFromPairingState($true); le = $true }
  )
  foreach ($src in $sources) {
    $infos = Await ([Windows.Devices.Enumeration.DeviceInformation]::FindAllAsync($src.sel)) ([Windows.Devices.Enumeration.DeviceInformationCollection])
    foreach ($i in $infos) {
      if (-not $i.Name -or $seen.ContainsKey($i.Name)) { continue }
      $connected = $false; $kind = ''
      try {
        if ($src.le) {
          $d = Await ([Windows.Devices.Bluetooth.BluetoothLEDevice]::FromIdAsync($i.Id)) ([Windows.Devices.Bluetooth.BluetoothLEDevice]); $kind = 'LE'
        } else {
          $d = Await ([Windows.Devices.Bluetooth.BluetoothDevice]::FromIdAsync($i.Id)) ([Windows.Devices.Bluetooth.BluetoothDevice]); $kind = "$($d.ClassOfDevice.MajorClass)"
        }
        if ($d) { $connected = ("$($d.ConnectionStatus)" -eq 'Connected'); $d.Dispose() }
      } catch {}
      $seen[$i.Name] = $true
      $out += @{ name = $i.Name; connected = $connected; kind = $kind }
    }
  }
  return $out
}

$script:mgr = $null
function Get-Session {
  if (-not $script:mgr) {
    $script:mgr = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
  }
  return $script:mgr.GetCurrentSession()
}
function Get-Media {
  $s = Get-Session
  if (-not $s) { return $null }
  $p = Await ($s.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
  $pb = $s.GetPlaybackInfo()
  return @{ title = $p.Title; artist = $p.Artist; app = $s.SourceAppUserModelId; status = "$($pb.PlaybackStatus)" }
}
function Invoke-Media([string]$action) {
  $s = Get-Session
  if (-not $s) { return $false }
  $op = switch ($action) { 'toggle' { $s.TryTogglePlayPauseAsync() } 'next' { $s.TrySkipNextAsync() } 'prev' { $s.TrySkipPreviousAsync() } }
  return (Await $op ([bool]))
}

$ThemeKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
function Get-Dark { try { return ((Get-ItemProperty $ThemeKey).AppsUseLightTheme -eq 0) } catch { return $false } }
function Set-Dark([bool]$dark) {
  $v = if ($dark) { 0 } else { 1 }
  Set-ItemProperty $ThemeKey -Name AppsUseLightTheme -Value $v -Type DWord
  Set-ItemProperty $ThemeKey -Name SystemUsesLightTheme -Value $v -Type DWord
  [MC.Sys]::BroadcastTheme()
  return $dark
}

function Get-Brightness {
  $list = @([MC.Display]::List() | ForEach-Object { @{ index = $_.Index; name = $_.Name; min = $_.Min; cur = $_.Cur; max = $_.Max } })
  if ($list.Count -eq 0) {
    # Laptop panels expose brightness through WMI instead of DDC/CI
    try {
      $w = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1
      if ($w) { $list = @(@{ index = -1; name = 'Built-in display'; min = 0; cur = [int]$w.CurrentBrightness; max = 100 }) }
    } catch {}
  }
  return $list
}
function Set-Brightness([int]$index, [int]$value) {
  if ($index -lt 0) {
    $m = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods | Select-Object -First 1
    Invoke-CimMethod -InputObject $m -MethodName WmiSetBrightness -Arguments @{ Timeout = 1; Brightness = [byte]$value } | Out-Null
    return $true
  }
  return [MC.Display]::Set($index, $value)
}

function Audio-State {
  return @{ out = @([MC.Audio]::List(0) | ForEach-Object { Dev $_ }); in = @([MC.Audio]::List(1) | ForEach-Object { Dev $_ }) }
}
function Dev($d) { return @{ id = $d.Id; name = $d.Name; isDefault = $d.IsDefault; volume = $d.Volume; muted = $d.Muted } }

[Console]::Out.WriteLine('{"ready":true}')
[Console]::Out.Flush()

while ($true) {
  $line = [Console]::In.ReadLine()
  if ($null -eq $line) { break }
  $line = $line.Trim().TrimStart([char]0xFEFF)
  if (-not $line) { continue }
  $req = $null
  try {
    $req = $line | ConvertFrom-Json
    $a = $req.args
    $data = $null
    switch ($req.cmd) {
      'state' {
        $media = $null; try { $media = Get-Media } catch {}
        $radios = @(); try { $radios = Get-Radios } catch {}
        $data = @{ audio = (Audio-State); radios = $radios; media = $media; dark = (Get-Dark) }
      }
      'bt' { $data = @{ devices = @(Get-BtDevices) } }
      'brightness' { $data = @{ monitors = @(Get-Brightness) } }
      'setVolume' { $data = [MC.Audio]::SetVolume([string]$a.id, [int]$a.value) }
      'stepVolume' { $data = [MC.Audio]::StepVolume([int]$a.delta) }
      'setMute' { $data = [MC.Audio]::SetMute([string]$a.id, [bool]$a.muted) }
      'setDefault' { $data = [MC.Audio]::SetDefault([string]$a.id) }
      'setRadio' { $data = Set-Radio ([string]$a.kind) ([bool]$a.on) }
      'media' { $data = Invoke-Media ([string]$a.action) }
      'setBrightness' { $data = Set-Brightness ([int]$a.index) ([int]$a.value) }
      'setDark' { $data = Set-Dark ([bool]$a.dark) }
      'lock' { $data = [MC.Sys]::LockWorkStation() }
      'sleep' { $data = [MC.Sys]::SetSuspendState($false, $false, $false) }
      default { throw "Unknown command $($req.cmd)" }
    }
    $resp = @{ id = $req.id; ok = $true; data = $data }
  } catch {
    $rid = 0; if ($req) { $rid = $req.id }
    $resp = @{ id = $rid; ok = $false; error = $_.Exception.Message }
  }
  [Console]::Out.WriteLine((ConvertTo-Json -InputObject $resp -Compress -Depth 6))
  [Console]::Out.Flush()
}
