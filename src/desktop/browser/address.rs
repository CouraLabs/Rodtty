//! What the Browser accepts as an address, deterministically:
//!
//! * `http://…` and `https://…` stay as typed (scheme lowercased).
//! * A bare local development address uses HTTP: `localhost`, `*.localhost`,
//!   `127.x.x.x`, `0.0.0.0`, `[::1]`, with or without a port and path.
//! * Any other bare host uses HTTPS: `docs.rs/cef` → `https://docs.rs/cef`.
//! * Every other scheme (`file:`, `javascript:`, `data:`, `chrome:`, custom
//!   schemes), text with spaces, and empty input are rejected. There is no
//!   web search.

pub fn normalize(input: &str) -> Result<String, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("Enter an address".into());
    }
    if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Addresses can't contain spaces; web search isn't supported".into());
    }
    if let Some((scheme, rest)) = text.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(format!(
                "{scheme}: addresses aren't allowed; use http or https"
            ));
        }
        if rest.is_empty() || rest.starts_with('/') {
            return Err("The address has no host".into());
        }
        return Ok(format!("{scheme}://{rest}"));
    }
    // `about:blank`, `mailto:x` and `localhost:4200` all have a colon; only
    // a host followed by a numeric port is an address without a scheme.
    let host_end = text.find(['/', '?', '#']).unwrap_or(text.len());
    let authority = &text[..host_end];
    let (host, port) = split_port(authority);
    if let Some(port) = port
        && (port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!(
            "{}: addresses aren't allowed; use http or https",
            text.split(':').next().unwrap_or(text)
        ));
    }
    if host.is_empty() {
        return Err("The address has no host".into());
    }
    let scheme = if is_local(host) { "http" } else { "https" };
    Ok(format!("{scheme}://{text}"))
}

fn split_port(authority: &str) -> (&str, Option<&str>) {
    if let Some(rest) = authority.strip_prefix('[') {
        // [::1]:8080
        return match rest.split_once(']') {
            Some((host, tail)) => (&authority[..host.len() + 2], tail.strip_prefix(':')),
            None => (authority, None),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    }
}

/// Whether `host` names this machine.
pub fn is_local(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "0.0.0.0"
        || host == "[::1]"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Whether a navigation the page or a redirect starts may proceed: only
/// HTTP(S) and the internal blank page.
pub fn allowed_navigation(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower == "about:blank"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_addresses_use_http_and_public_hosts_https() {
        assert_eq!(
            normalize("localhost:4200").unwrap(),
            "http://localhost:4200"
        );
        assert_eq!(
            normalize("127.0.0.1:8080/app").unwrap(),
            "http://127.0.0.1:8080/app"
        );
        assert_eq!(normalize("[::1]:3000").unwrap(), "http://[::1]:3000");
        assert_eq!(normalize("api.localhost").unwrap(), "http://api.localhost");
        assert_eq!(normalize("docs.rs/cef").unwrap(), "https://docs.rs/cef");
        assert_eq!(
            normalize("example.com:8443").unwrap(),
            "https://example.com:8443"
        );
        assert_eq!(
            normalize(" HTTP://Example.com ").unwrap(),
            "http://Example.com"
        );
        assert_eq!(
            normalize("http://[::1]:5000/").unwrap(),
            "http://[::1]:5000/"
        );
    }

    #[test]
    fn other_schemes_spaces_and_empty_input_are_rejected() {
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
            "chrome://settings",
            "vscode://open",
            "mailto:a@b.c",
            "",
            "two words",
            "about:config",
            "http://",
        ] {
            assert!(normalize(bad).is_err(), "{bad}");
        }
        assert!(allowed_navigation("https://x"));
        assert!(allowed_navigation("about:blank"));
        assert!(!allowed_navigation("file:///etc/hosts"));
        assert!(!allowed_navigation("custom-app://x"));
    }
}
