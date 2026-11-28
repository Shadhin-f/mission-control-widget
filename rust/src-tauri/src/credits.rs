// Credits tab: the project's GitHub contributors from the public API, cached for a day in the settings
// folder (the same contributors.json the pure Rust edition writes, so both share it).
use serde_json::{json, Value};
use std::path::PathBuf;

pub const REPO: &str = "Shadhin-f/mission-control-widget";
const DAY_MS: f64 = 24.0 * 3600.0 * 1000.0;

fn dir() -> PathBuf {
    std::env::var("MC_SETTINGS_DIR").map(PathBuf::from).unwrap_or_else(|_| dirs::config_dir().unwrap_or_else(crate::util::home).join("Mission Control"))
}
fn cache_file() -> PathBuf {
    dir().join("contributors.json")
}
/// Same file names as the pure Rust edition's avatar cache
fn avatar_file(login: &str) -> PathBuf {
    let safe: String = login.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    dir().join("avatars").join(format!("{safe}.img"))
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i)) as usize & 63] as char } else { '=' });
        }
    }
    out
}

/// A cached avatar as a data: URL the page can show without any network access.
fn avatar_data(login: &str) -> Option<String> {
    let b = std::fs::read(avatar_file(login)).ok().filter(|b| !b.is_empty())?;
    let mime = if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if b.starts_with(b"GIF") {
        "image/gif"
    } else if b.len() > 12 && &b[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };
    Some(format!("data:{mime};base64,{}", base64(&b)))
}

/// Downloads any avatars that aren't cached yet, then swaps every remote URL for its cached copy.
async fn with_avatars(mut people: Vec<Value>, refetch: bool) -> Vec<Value> {
    let http = reqwest::Client::builder().user_agent("MissionControl/1.0").timeout(std::time::Duration::from_secs(10)).build().ok();
    for p in people.iter_mut() {
        let login = p["login"].as_str().unwrap_or("").to_string();
        let url = p["avatar"].as_str().unwrap_or("").to_string();
        let file = avatar_file(&login);
        if (refetch || !file.exists()) && !url.is_empty() {
            if let Some(http) = &http {
                if let Ok(r) = http.get(&url).send().await {
                    if r.status().is_success() {
                        if let Ok(b) = r.bytes().await {
                            if let Some(d) = file.parent() {
                                let _ = std::fs::create_dir_all(d);
                            }
                            let _ = std::fs::write(&file, &b);
                        }
                    }
                }
            }
        }
        p["avatar"] = json!(avatar_data(&login).unwrap_or_default());
    }
    people
}

/// GitHub's contributor list -> [{ login, url, avatar, commits }], without bots.
fn people(list: &Value) -> Vec<Value> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["type"] != "Bot" && !c["login"].as_str().unwrap_or("").ends_with("[bot]") && c["login"].is_string())
        .map(|c| {
            let avatar = c["avatar_url"].as_str().unwrap_or("");
            json!({
                "login": c["login"],
                "url": c["html_url"],
                "avatar": if avatar.is_empty() { String::new() } else { format!("{avatar}{}s=96", if avatar.contains('?') { "&" } else { "?" }) },
                "commits": c["contributions"].as_u64().unwrap_or(0),
            })
        })
        .collect()
}

/// Shown until the first successful fetch (offline first run)
fn fallback() -> Vec<Value> {
    ["Shadhin-f", "Awsaf2745"].iter().map(|l| json!({ "login": l, "url": format!("https://github.com/{l}"), "avatar": "", "commits": 0 })).collect()
}

/// The contributors, refreshed from GitHub at most once a day (or now, with `force`).
pub async fn get(force: bool) -> Value {
    let cached = std::fs::read_to_string(cache_file()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let at = cached.as_ref().and_then(|c| c["at"].as_f64()).unwrap_or(0.0);
    let list = cached.as_ref().map(|c| people(&c["list"])).unwrap_or_default();
    if !force && !list.is_empty() && crate::util::now_ms() - at < DAY_MS {
        return json!({ "repo": REPO, "people": with_avatars(list, false).await, "at": at, "error": null });
    }
    let fetched = async {
        let http = reqwest::Client::builder().user_agent("MissionControl/1.0").timeout(std::time::Duration::from_secs(10)).build().map_err(|e| e.to_string())?;
        let r = http.get(format!("https://api.github.com/repos/{REPO}/contributors?per_page=100")).header("Accept", "application/vnd.github+json").send().await.map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("GitHub answered {}", r.status()));
        }
        r.json::<Value>().await.map_err(|e| e.to_string())
    }
    .await;
    match fetched {
        Ok(v) => {
            let now = crate::util::now_ms();
            if let Some(dir) = cache_file().parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(cache_file(), json!({ "at": now, "list": v }).to_string());
            // a fresh list may come with new avatars: download them again
            json!({ "repo": REPO, "people": with_avatars(people(&v), true).await, "at": now, "error": null })
        }
        Err(e) => {
            let people = if list.is_empty() { fallback() } else { list };
            json!({ "repo": REPO, "people": with_avatars(people, false).await, "at": at, "error": e })
        }
    }
}
