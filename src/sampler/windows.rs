//! Hyprland's window list, read straight from its socket (no process spawn).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Window {
    pub pid: i32,
    pub class: String,
    pub title: String,
    pub address: String,
}

fn socket() -> Option<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    Some(runtime.join("hypr").join(sig).join(".socket.sock"))
}

/// Send one request to Hyprland and return the reply.
pub fn request(req: &str) -> Option<String> {
    let mut s = UnixStream::connect(socket()?).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn parse_clients(text: &str) -> Vec<Window> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(text) else { return vec![] };
    list.iter()
        .filter(|c| c["mapped"].as_bool().unwrap_or(true))
        .filter_map(|c| {
            Some(Window {
                pid: c["pid"].as_i64()? as i32,
                class: c["class"].as_str().unwrap_or("").to_string(),
                title: c["title"].as_str().unwrap_or("").to_string(),
                address: c["address"].as_str().unwrap_or("").to_string(),
            })
        })
        .filter(|w| w.pid > 0)
        .collect()
}

pub fn clients() -> Vec<Window> {
    request("j/clients").map(|t| parse_clients(&t)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clients() {
        let text = r#"[{"address":"0x1","mapped":true,"pid":42,"class":"firefox","title":"Hi","workspace":{"id":1,"name":"1"}},
                      {"address":"0x2","mapped":true,"pid":-1,"class":"x","title":"","workspace":{"id":1,"name":"1"}}]"#;
        let w = parse_clients(text);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].pid, 42);
        assert_eq!(w[0].class, "firefox");
    }
}
