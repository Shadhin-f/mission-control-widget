// Small helpers shared by every module: running commands without a console window, timestamps.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A std Command that never flashes a console window on Windows.
pub fn std_cmd(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// A tokio Command that never flashes a console window on Windows.
pub fn cmd(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut c = tokio::process::Command::new(program);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// `.cmd` shims (npm installs) need cmd.exe to run.
pub fn cli_cmd(exe: &Path) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let s = exe.to_string_lossy().to_lowercase();
        if s.ends_with(".cmd") || s.ends_with(".bat") {
            let mut c = cmd("cmd.exe");
            c.arg("/d").arg("/s").arg("/c").arg(exe);
            return c;
        }
    }
    cmd(exe)
}

pub struct RunOut {
    pub ok: bool,
    pub out: String,
    pub err: String,
}

/// Runs a program to completion with a timeout (like Node's execFile).
pub async fn run(program: &str, args: &[&str], timeout_ms: u64) -> RunOut {
    let mut c = cmd(program);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let child = match c.spawn() {
        Ok(c) => c,
        Err(e) => return RunOut { ok: false, out: String::new(), err: e.to_string() },
    };
    match tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await {
        Ok(Ok(o)) => RunOut {
            ok: o.status.success(),
            out: decode(&o.stdout),
            err: decode(&o.stderr),
        },
        Ok(Err(e)) => RunOut { ok: false, out: String::new(), err: e.to_string() },
        Err(_) => RunOut { ok: false, out: String::new(), err: "Timed out".into() },
    }
}

/// Console tools print UTF-8 or the OEM code page; keep whatever is valid.
pub fn decode(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn env_path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

/// Kills a child and everything it started.
pub fn stop_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = std_cmd("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    #[cfg(unix)]
    unsafe {
        if libc::kill(-(pid as i32), libc::SIGTERM) != 0 {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
}

/// Finds an executable on PATH and the usual per-user install folders (macOS / Linux).
#[cfg(unix)]
pub fn find_unix_cli(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let h = home();
    let mut dirs = env_path_dirs();
    dirs.extend([h.join(".local/bin"), h.join(".npm-global/bin"), "/opt/homebrew/bin".into(), "/usr/local/bin".into(), "/usr/bin".into()]);
    dirs.into_iter().map(|d| d.join(name)).find(|p| std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false))
}
