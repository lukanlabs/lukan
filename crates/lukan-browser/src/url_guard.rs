//! SSRF prevention — blocks navigation to private/internal addresses.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use url::Url;

/// Global flag: when true, allow navigation to internal/private IPs.
static ALLOW_INTERNAL: AtomicBool = AtomicBool::new(false);

/// Set whether internal/private IPs are allowed.
pub fn set_allow_internal(allow: bool) {
    ALLOW_INTERNAL.store(allow, Ordering::Relaxed);
}

/// Check whether internal/private IPs are currently allowed.
pub fn is_internal_allowed() -> bool {
    ALLOW_INTERNAL.load(Ordering::Relaxed)
}

/// Check a URL for SSRF risks.
///
/// Returns `Some(error_message)` if the URL is blocked, `None` if it's OK.
pub fn check_url(raw_url: &str) -> Option<String> {
    let url = match Url::parse(raw_url) {
        Ok(u) => u,
        Err(e) => return Some(format!("Invalid URL: {e}")),
    };

    let scheme = url.scheme();

    // Allow Chrome internal URLs (about:blank, chrome:, data:)
    if scheme == "about" || scheme == "chrome" {
        return None;
    }

    if scheme != "http" && scheme != "https" {
        return Some(format!("Unsupported scheme: {scheme}"));
    }

    if is_internal_allowed() {
        return None;
    }

    let host = match url.host_str() {
        Some(h) => h,
        None => return Some("URL has no host".to_string()),
    };

    if is_blocked_host(host) {
        Some(format!(
            "Access to '{host}' is blocked for security (SSRF prevention). \
             Use --browser-allow-internal to override."
        ))
    } else {
        None
    }
}

fn is_blocked_host(host: &str) -> bool {
    // Exact hostname matches
    let lower = host.to_lowercase();
    if lower == "localhost"
        || lower == "metadata.google.internal"
        || lower == "instance-data"
        || lower.ends_with(".localhost")
    {
        return true;
    }

    // IP address checks
    // Strip brackets from IPv6 like [::1]
    let ip_str = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = ip_str.parse::<IpAddr>() {
        return is_private_ip(ip);
    }

    false
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()        // 127.0.0.0/8
                || v4.is_private()      // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()   // 169.254.0.0/16
                || v4.is_unspecified()  // 0.0.0.0
                || v4.octets()[0] == 0 // 0.0.0.0/8
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()    // ::1
                || v6.is_unspecified() // ::
                // fe80::/10 (link-local) — check first two bytes
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_urls() {
        assert!(check_url("https://example.com").is_none());
        assert!(check_url("https://www.google.com/search?q=test").is_none());
        assert!(check_url("http://93.184.216.34").is_none());
    }

    #[test]
    fn blocks_localhost() {
        set_allow_internal(false);
        assert!(check_url("http://localhost").is_some());
        assert!(check_url("http://localhost:3000").is_some());
        assert!(check_url("http://sub.localhost").is_some());
    }

    #[test]
    fn blocks_private_ipv4() {
        set_allow_internal(false);
        assert!(check_url("http://127.0.0.1").is_some());
        assert!(check_url("http://10.0.0.1").is_some());
        assert!(check_url("http://172.16.0.1").is_some());
        assert!(check_url("http://172.31.255.255").is_some());
        assert!(check_url("http://192.168.1.1").is_some());
        assert!(check_url("http://169.254.169.254").is_some());
        assert!(check_url("http://0.0.0.0").is_some());
    }

    #[test]
    fn blocks_ipv6_loopback() {
        set_allow_internal(false);
        assert!(check_url("http://[::1]").is_some());
    }

    #[test]
    fn blocks_cloud_metadata() {
        set_allow_internal(false);
        assert!(check_url("http://metadata.google.internal").is_some());
        assert!(check_url("http://instance-data").is_some());
    }

    #[test]
    fn blocks_non_http_schemes() {
        assert!(check_url("ftp://example.com").is_some());
        assert!(check_url("file:///etc/passwd").is_some());
    }

    #[test]
    fn allows_public_ipv4() {
        // 172.32.x.x is NOT private (private range is 172.16-31)
        assert!(check_url("http://172.32.0.1").is_none());
        assert!(check_url("http://8.8.8.8").is_none());
    }

    #[test]
    fn allow_internal_flag_overrides() {
        // This test modifies global state (AtomicBool), so we only test
        // the "allow" direction to avoid races with other tests that
        // set_allow_internal(false). The "block" behavior is tested by
        // blocks_localhost and blocks_private_ipv4.
        set_allow_internal(true);
        assert!(check_url("http://localhost").is_none());
        assert!(check_url("http://192.168.1.1").is_none());
        // Restore default
        set_allow_internal(false);
    }
}
