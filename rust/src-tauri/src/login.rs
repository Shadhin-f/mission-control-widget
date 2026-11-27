// Launch at sign-in.
// Windows: a Task Scheduler "at log on" task (starts right at sign-in, while Run-key entries and
// Startup-folder shortcuts wait behind every other startup app), falling back to a Startup shortcut.
// macOS: a LaunchAgent.
/// The scheduled task's name. MC_LOGIN_TASK overrides it so a test run never replaces the real entry.
#[cfg(windows)]
fn task_name() -> String {
    std::env::var("MC_LOGIN_TASK").unwrap_or_else(|_| "Mission Control".into())
}

#[cfg(windows)]
async fn ps(script: &str, env: &[(&str, String)]) -> String {
    let mut c = crate::util::cmd("powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]).stdin(std::process::Stdio::null()).kill_on_drop(true);
    for (k, v) in env {
        c.env(k, v);
    }
    match tokio::time::timeout(std::time::Duration::from_secs(30), c.output()).await {
        Ok(Ok(o)) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

#[cfg(windows)]
fn startup_lnk() -> std::path::PathBuf {
    dirs::data_dir().unwrap_or_default().join("Microsoft/Windows/Start Menu/Programs/Startup/Mission Control.lnk")
}

pub async fn apply(open_at_login: bool) {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    #[cfg(windows)]
    {
        let env = [("MC_EXE", exe.to_string_lossy().to_string()), ("MC_ARGS", String::new()), ("MC_DIR", dir.to_string_lossy().to_string()), ("MC_TASK", task_name()), ("MC_LNK", startup_lnk().to_string_lossy().to_string())];
        let remove_shortcut = || {
            let _ = std::fs::remove_file(startup_lnk());
        };
        if !open_at_login {
            remove_shortcut();
            ps("Unregister-ScheduledTask -TaskName $env:MC_TASK -Confirm:$false -ErrorAction SilentlyContinue", &env).await;
            return;
        }
        // Skip the (slow) registration when the task already points at this copy
        let check = ps("$t = Get-ScheduledTask -TaskName $env:MC_TASK -ErrorAction SilentlyContinue; if ($t -and $t.Actions[0].Execute -eq $env:MC_EXE -and \"$($t.Actions[0].Arguments)\" -eq $env:MC_ARGS -and $t.Settings.Priority -eq 4) { 'same' }", &env).await;
        if check == "same" {
            remove_shortcut();
            return;
        }
        let reg = ps(
            &[
                "$u = \"$env:USERDOMAIN\\$env:USERNAME\"",
                "$a = New-ScheduledTaskAction -Execute $env:MC_EXE -WorkingDirectory $env:MC_DIR",
                "$t = New-ScheduledTaskTrigger -AtLogOn -User $u",
                "$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -Priority 4 -MultipleInstances IgnoreNew",
                "$p = New-ScheduledTaskPrincipal -UserId $u -LogonType Interactive -RunLevel Limited",
                "Register-ScheduledTask -TaskName $env:MC_TASK -Description 'Starts the Mission Control widget at sign-in' -Action $a -Trigger $t -Settings $s -Principal $p -Force -ErrorAction Stop | Out-Null",
                "'ok'",
            ]
            .join("; "),
            &env,
        )
        .await;
        if reg.ends_with("ok") {
            remove_shortcut();
        } else {
            // Fallback: slower, but always works
            ps("$w = New-Object -ComObject WScript.Shell; $l = $w.CreateShortcut($env:MC_LNK); $l.TargetPath = $env:MC_EXE; $l.WorkingDirectory = $env:MC_DIR; $l.Description = 'Mission Control widget'; $l.Save()", &env).await;
        }
    }
    #[cfg(target_os = "macos")]
    {
        let file = crate::util::home().join("Library/LaunchAgents/com.missioncontrol.widget.plist");
        if !open_at_login {
            let _ = std::fs::remove_file(&file);
            return;
        }
        let xml = |v: &str| v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;");
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin".into());
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>Label</key><string>com.missioncontrol.widget</string><key>ProgramArguments</key><array><string>{}</string></array><key>RunAtLoad</key><true/><key>WorkingDirectory</key><string>{}</string><key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string></dict></dict></plist>"#,
            xml(&exe.to_string_lossy()),
            xml(&dir.to_string_lossy()),
            xml(&path)
        );
        if let Some(p) = file.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&file, body);
        let _ = crate::util::run("/usr/bin/plutil", &["-lint", &file.to_string_lossy()], 5000).await;
    }
}
