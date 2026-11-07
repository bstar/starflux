//! Explicit URL routing to reviewed plugin actions. Resolution never executes code.
use crate::plugins::{self, LinkHandler};
use anyhow::{Result, ensure};
use gio::glib::{Uri, UriFlags};
use serde_json::{Value, json};

fn scheme(value: &str) -> bool {
    value.len() <= 32
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
}
fn host(value: &str) -> bool {
    if value.parse::<std::net::Ipv6Addr>().is_ok() {
        return true;
    }
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
pub fn validate_handler(handler: &LinkHandler) -> Result<()> {
    ensure!(
        !handler.title.is_empty()
            && handler.title.len() <= 128
            && !handler.title.chars().any(char::is_control),
        "Invalid link handler title"
    );
    ensure!(
        !handler.schemes.is_empty()
            && handler.schemes.len() <= 16
            && handler.schemes.iter().all(|s| scheme(s)),
        "Invalid link handler schemes"
    );
    ensure!(
        handler.hosts.len() <= 32 && handler.hosts.iter().all(|s| host(s)),
        "Link handler hosts must be exact DNS names or IP addresses, without ports"
    );
    Ok(())
}
pub fn parse(value: &str) -> Result<Uri> {
    ensure!(
        !value.is_empty()
            && value.len() <= 8192
            && !value.chars().any(|c| c.is_control() || c.is_whitespace()),
        "Enter an absolute URL, up to 8192 bytes; encode spaces"
    );
    let uri = Uri::parse(value, UriFlags::ENCODED)?;
    ensure!(
        scheme(&uri.scheme()) && uri.userinfo().is_none(),
        "Links must have a scheme and must not contain credentials"
    );
    Ok(uri)
}
pub fn matches(handler: &LinkHandler, uri: &Uri) -> bool {
    handler
        .schemes
        .iter()
        .any(|s| s.eq_ignore_ascii_case(&uri.scheme()))
        && (handler.hosts.is_empty()
            || uri
                .host()
                .is_some_and(|h| handler.hosts.iter().any(|s| s.eq_ignore_ascii_case(&h))))
}
pub fn resolve(url: &str, offset: usize, limit: usize) -> Result<Value> {
    ensure!(
        (1..=128).contains(&limit),
        "Link handler page size must be 1–128"
    );
    let uri = parse(url)?;
    let registry = plugins::registry()?;
    let mut handlers = Vec::new();
    let mut warnings = Vec::new();
    for (id, entry) in &registry.plugins {
        if !entry.enabled {
            continue;
        }
        match plugins::package_for(entry) {
            Ok((manifest, digest))
                if manifest.id == *id && entry.approved.as_deref() == Some(&digest) =>
            {
                for handler in &manifest.link_handlers {
                    if matches(handler, &uri) {
                        handlers.push(json!({"plugin":id,"plugin_name":manifest.name,
                            "handler":handler,"digest":digest}));
                    }
                }
            }
            Ok(_) => warnings
                .push(json!({"plugin":id,"warning":"Package changed; review and enable it again"})),
            Err(error) => warnings
                .push(json!({"plugin":id,"warning":format!("Package unavailable: {error:#}")})),
        }
    }
    let total = handlers.len();
    let next = (offset.saturating_add(limit) < total).then(|| offset.saturating_add(limit));
    let handlers = handlers
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    Ok(json!({"url":url,"handlers":handlers,"warnings":warnings,
        "offset":offset,"limit":limit,"total":total,"next_offset":next,
        "revision":registry.revision,"execution_host":"workspace_controller"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_matching_uses_exact_parsed_hosts_and_literal_context() {
        let handler = LinkHandler {
            id: "open".into(),
            title: "Open issue".into(),
            action: "show".into(),
            schemes: vec!["HTTPS".into()],
            hosts: vec!["example.com".into()],
        };
        validate_handler(&handler).unwrap();
        for url in [
            "https://EXAMPLE.com:8443/a?q=%24%28touch%20never%29#fragment",
            "HTTPS://example.com/界",
        ] {
            assert!(matches(&handler, &parse(url).unwrap()));
        }
        for url in [
            "https://example.com.evil.invalid/",
            "https://evil.invalid/example.com",
            "http://example.com/",
        ] {
            assert!(!matches(&handler, &parse(url).unwrap()));
        }
        for url in [
            "https://user:pass@example.com/",
            "https://example.com/a b",
            "https://example.com/\x1b",
            "/relative/path",
        ] {
            assert!(parse(url).is_err());
        }
        let custom = LinkHandler {
            schemes: vec!["sf-issue".into()],
            hosts: vec![],
            ..handler
        };
        assert!(matches(&custom, &parse("sf-issue:project/123").unwrap()));
        for value in ["*.example.com", "example.com:80", "-bad.com", "", "bad/com"] {
            assert!(!host(value));
        }
    }
}
