//! Remote opencode servers must use HTTPS unless they are on loopback (tunnels land on loopback) (R-35).
//! Port of `remoteUrlAllowed` and `Supervisor.testRemote`.

use super::client::OpencodeClient;
use serde_json::{json, Value};

pub const NOT_A_URL: &str = "Not a URL — use http:// or https://";
pub const NEEDS_HTTPS: &str = "Remote servers must be reached over HTTPS or an SSH tunnel to localhost";
/// Not in the TS; a URL with embedded credentials would land the password in `harness.json`.
pub const NO_CREDENTIALS: &str = "Do not put credentials in the URL — set the username and password instead";

fn split_authority(rest: &str) -> (&str, &str) {
    match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    }
}

/// `host` and `port` of an authority, handling `[::1]:4096` and `::1`.
fn host_port(authority: &str) -> (&str, &str) {
    if let Some(rest) = authority.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':').unwrap_or("")),
            None => (authority, ""),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(':') => (host, port),
        _ => (authority, ""),
    }
}

/// Same verdicts as the TS: `https` anywhere, plain `http` only on loopback, plus no userinfo.
pub fn remote_url_allowed(url: &str) -> Result<(), &'static str> {
    let Some((scheme, rest)) = url.split_once("://") else { return Err(NOT_A_URL) };
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return Err(NEEDS_HTTPS);
    }
    let (authority, _) = split_authority(rest);
    if authority.contains('@') {
        return Err(NO_CREDENTIALS);
    }
    if scheme == "https" {
        return Ok(());
    }
    let (host, _) = host_port(authority);
    if ["127.0.0.1", "localhost", "::1"].contains(&host.to_ascii_lowercase().as_str()) {
        return Ok(());
    }
    Err(NEEDS_HTTPS)
}

/// `new URL(url).port` as an i64 (0 when absent).
pub fn port_of(url: &str) -> i64 {
    let Some((_, rest)) = url.split_once("://") else { return 0 };
    host_port(split_authority(rest).0).1.parse().unwrap_or(0)
}

pub struct RemoteTest {
    pub ok: bool,
    pub version: Option<String>,
    pub sessions: Option<usize>,
    pub folders: Option<usize>,
    pub providers: Option<usize>,
    pub error: Option<String>,
}

impl RemoteTest {
    pub fn to_json(&self) -> Value {
        let mut out = json!({ "ok": self.ok });
        for (k, v) in [("version", &self.version), ("error", &self.error)] {
            if let Some(v) = v {
                out[k] = json!(v);
            }
        }
        for (k, v) in [("sessions", self.sessions), ("folders", self.folders), ("providers", self.providers)] {
            if let Some(v) = v {
                out[k] = json!(v);
            }
        }
        out
    }
}

/// Test a remote server before connecting (onboarding and Settings › Engine).
pub fn test_remote(url: &str, username: &str, password: &str) -> RemoteTest {
    let fail = |error: String| RemoteTest { ok: false, version: None, sessions: None, folders: None, providers: None, error: Some(error) };
    if let Err(reason) = remote_url_allowed(url) {
        return fail(reason.into());
    }
    let client = OpencodeClient::new(url, if username.is_empty() { "opencode" } else { username }, password);
    let info = match client.info() {
        Ok(info) => info,
        Err(e) => {
            let error = if e.to_string().contains("401") {
                "The password was rejected.".to_string()
            } else {
                "Can’t reach that server — check the URL and the password.".to_string()
            };
            return fail(error);
        }
    };
    let sessions = client.list_sessions(None, 500).unwrap_or_default();
    let providers = client.providers().map(|p| p.len()).unwrap_or(0);
    let folders: Vec<String> = sessions.iter().filter_map(|s| s["location"]["directory"].as_str().map(str::to_string)).collect();
    let unique: std::collections::BTreeSet<&String> = folders.iter().collect();
    RemoteTest { ok: true, version: Some(info.version), sessions: Some(sessions.len()), folders: Some(unique.len()), providers: Some(providers), error: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_https_anywhere_and_http_only_on_loopback() {
        assert!(remote_url_allowed("https://oc.example.com").is_ok());
        assert!(remote_url_allowed("http://127.0.0.1:4096").is_ok());
        assert!(remote_url_allowed("http://localhost:4096").is_ok());
        assert!(remote_url_allowed("http://[::1]:4096").is_ok());
        assert!(remote_url_allowed("http://10.0.0.5:4096").is_err());
        assert!(remote_url_allowed("http://example.com").is_err());
        assert!(remote_url_allowed("ftp://example.com").is_err());
        assert!(remote_url_allowed("not a url").is_err());
        assert_eq!(remote_url_allowed("http://10.0.0.5:4096").unwrap_err(), NEEDS_HTTPS);
        assert_eq!(remote_url_allowed("not a url").unwrap_err(), NOT_A_URL);
        assert_eq!(remote_url_allowed("http://user:pw@localhost:4096").unwrap_err(), NO_CREDENTIALS);
    }

    #[test]
    fn reads_the_port_like_url_port() {
        assert_eq!(port_of("http://127.0.0.1:4096"), 4096);
        assert_eq!(port_of("https://oc.example.com/path"), 0);
        assert_eq!(port_of("http://[::1]:8080/x"), 8080);
    }
}
