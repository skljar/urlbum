use std::io::Read;
use std::path::Path;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Fetch favicon for a URL, save to favicons_dir as `{domain}.png`, return filename.
pub fn fetch_favicon(url: &str, favicons_dir: &Path) -> Option<String> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }

    let domain = extract_domain(url)?;
    let safe = sanitize_domain(&domain);
    let filename = format!("{safe}.png");
    let dest = favicons_dir.join(&filename);

    // Cache hit — validate PNG magic bytes
    if dest.exists() {
        if let Ok(cached) = std::fs::read(&dest) {
            if is_valid_image(&cached) {
                return Some(filename);
            }
            let _ = std::fs::remove_file(&dest); // corrupt cache — re-fetch
        }
    }

    let _ = std::fs::create_dir_all(favicons_dir);

    let try_save = |bytes: Vec<u8>| -> Option<()> {
        let saveable = prepare_image(bytes)?;
        std::fs::write(&dest, &saveable).ok()
    };

    // Strategy 1: /favicon.ico (HTTPS then HTTP)
    for scheme in &["https", "http"] {
        if let Some(bytes) = try_get(&format!("{scheme}://{domain}/favicon.ico")) {
            if try_save(bytes).is_some() {
                return Some(filename);
            }
        }
    }

    // Strategy 2: <link rel="icon"> in HTML
    let html_urls = [url.to_string(), format!("https://{domain}/"), format!("http://{domain}/")];
    'html: for fetch_url in &html_urls {
        if let Some(html_bytes) = try_get(fetch_url) {
            let html = String::from_utf8_lossy(&html_bytes);
            for icon_href in find_icon_links(&html, fetch_url) {
                if let Some(bytes) = try_get(&icon_href) {
                    if try_save(bytes).is_some() {
                        return Some(filename);
                    }
                }
            }
            if !html.is_empty() {
                break 'html;
            }
        }
    }

    // Strategy 3: DuckDuckGo favicon service (handles Cloudflare-protected sites)
    if let Some(bytes) = try_get(&format!("https://icons.duckduckgo.com/ip3/{domain}.ico")) {
        if try_save(bytes).is_some() {
            return Some(filename);
        }
    }

    // Strategy 4: Google S2 — last resort; reject 1×1 placeholder (≤68 bytes)
    if let Some(bytes) =
        try_get(&format!("https://www.google.com/s2/favicons?domain={domain}&sz=32"))
    {
        if bytes.len() > 68 {
            if try_save(bytes).is_some() {
                return Some(filename);
            }
        }
    }

    None
}

pub fn extract_domain(url: &str) -> Option<String> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let after_scheme = &url[url.find("://")? + 3..];
    let host = after_scheme.split('/').next()?;
    let host = host.split(':').next()?;
    let host = host.trim_start_matches("www.");
    if host.is_empty() { None } else { Some(host.to_lowercase()) }
}

pub fn sanitize_domain(domain: &str) -> String {
    domain
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

pub fn is_valid_image(bytes: &[u8]) -> bool {
    // All images are re-encoded to PNG, so only PNG magic is valid in cache
    bytes.len() >= 4 && bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47])
}

/// Decode any image format and re-encode as PNG (SVG rejected — Slint needs raster).
fn prepare_image(bytes: Vec<u8>) -> Option<Vec<u8>> {
    if bytes.len() < 4 {
        return None;
    }
    if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xm") {
        return None;
    }
    let img = image::load_from_memory(&bytes).ok()?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    if out.is_empty() { None } else { Some(out) }
}

fn try_get(url: &str) -> Option<Vec<u8>> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(8))
        .set("User-Agent", UA)
        .call()
        .ok()?;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf).ok()?;
    Some(buf)
}

pub fn find_icon_links(html: &str, base_url: &str) -> Vec<String> {
    let lower = html.to_lowercase();
    let patterns = [
        "rel=\"icon\"",
        "rel=\"shortcut icon\"",
        "rel='icon'",
        "rel='shortcut icon'",
        "rel=\"apple-touch-icon\"",
        "rel='apple-touch-icon'",
    ];
    let mut raster = Vec::new();
    let mut svg = Vec::new();
    let mut search_from = 0usize;
    loop {
        let found = patterns
            .iter()
            .filter_map(|pat| lower[search_from..].find(pat).map(|p| p + search_from))
            .min();
        let Some(pos) = found else { break };
        let start = lower[..pos].rfind('<').unwrap_or(0);
        let end = lower[pos..].find('>').map(|e| pos + e + 1).unwrap_or(html.len());
        let tag = &html[start..end];
        if let Some(href) = extract_attr(tag, "href") {
            let resolved = resolve_url(&href, base_url);
            let is_svg = href.to_lowercase().ends_with(".svg")
                || extract_attr(tag, "type").as_deref() == Some("image/svg+xml");
            if is_svg { svg.push(resolved); } else { raster.push(resolved); }
        }
        search_from = end.max(pos + 1);
    }
    raster.extend(svg); // raster first, SVG last (Slint loads PNG from path)
    raster
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    for q in ['"', '\''] {
        let needle = format!("{}={}", attr, q);
        if let Some(s) = lower.find(&needle) {
            let vs = s + needle.len();
            if let Some(e) = tag[vs..].find(q) {
                return Some(tag[vs..vs + e].to_string());
            }
        }
    }
    None
}

fn resolve_url(href: &str, base: &str) -> String {
    if href.starts_with("http") {
        return href.to_string();
    }
    if href.starts_with("//") {
        return format!("https:{href}");
    }
    if let Some(p) = base.find("://") {
        let after = &base[p + 3..];
        let host_end = after.find('/').unwrap_or(after.len());
        let host = &after[..host_end];
        if href.starts_with('/') {
            return format!("https://{host}{href}");
        }
        let base_dir = base.rfind('/').map(|p| &base[..p]).unwrap_or(base);
        return format!("{base_dir}/{href}");
    }
    href.to_string()
}

/// One fetch per unique domain. Other bookmarks sharing the domain go into `same_ids`.
pub fn dedup_by_domain(nodes: Vec<crate::db::Node>) -> Vec<(crate::db::Node, Vec<i64>)> {
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    let mut result: Vec<(crate::db::Node, Vec<i64>)> = Vec::new();
    for node in nodes {
        let domain = node.url.as_deref().and_then(extract_domain).unwrap_or_default();
        if let Some(&idx) = seen.get(&domain) {
            result[idx].1.push(node.id);
        } else {
            seen.insert(domain, result.len());
            let id = node.id;
            result.push((node, vec![id]));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: i64, url: &str) -> crate::db::Node {
        crate::db::Node {
            id,
            parent: None,
            kind: "bookmark".into(),
            title: "T".into(),
            url: Some(url.into()),
            note: None,
            sort_idx: 0,
            created: None,
            visited: None,
        }
    }

    #[test]
    fn extract_domain_strips_www_scheme_port_path() {
        assert_eq!(extract_domain("https://www.github.com/foo/bar"), Some("github.com".into()));
        assert_eq!(extract_domain("http://example.com:8080/page"), Some("example.com".into()));
        assert_eq!(
            extract_domain("https://sub.example.co.uk/"),
            Some("sub.example.co.uk".into())
        );
    }

    #[test]
    fn extract_domain_rejects_non_http() {
        assert_eq!(extract_domain("ftp://other.com"), None);
        assert_eq!(extract_domain("chrome://settings"), None);
        assert_eq!(extract_domain("about:blank"), None);
        assert_eq!(extract_domain("file:///C:/x.html"), None);
        assert_eq!(extract_domain("https://"), None);
    }

    #[test]
    fn sanitize_domain_allows_alphanum_dot_dash() {
        assert_eq!(sanitize_domain("github.com"), "github.com");
        assert_eq!(sanitize_domain("my-site.co.uk"), "my-site.co.uk");
        assert_eq!(sanitize_domain("x1.example.com"), "x1.example.com");
    }

    #[test]
    fn sanitize_domain_replaces_unsafe_chars() {
        assert_eq!(sanitize_domain("a_b c"), "a_b_c");
        assert_eq!(sanitize_domain("foo:bar"), "foo_bar");
    }

    #[test]
    fn is_valid_image_accepts_png_magic() {
        let png = &[0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(is_valid_image(png));
    }

    #[test]
    fn is_valid_image_rejects_non_png() {
        assert!(!is_valid_image(b"<html>not an image</html>"));
        assert!(!is_valid_image(b"\xFF\xD8\xFF")); // JPEG
        assert!(!is_valid_image(&[0x00u8, 0x00, 0x01, 0x00])); // ICO
        assert!(!is_valid_image(b"GIF89a")); // GIF
        assert!(!is_valid_image(b"RIF")); // too short
    }

    #[test]
    fn find_icon_links_raster_before_svg() {
        let html = r#"<head>
            <link rel="icon" href="/favicon.png" type="image/png">
            <link rel="icon" href="/favicon.svg" type="image/svg+xml">
            <link rel="apple-touch-icon" href="/apple.png">
        </head>"#;
        let links = find_icon_links(html, "https://example.com/");
        assert!(links.len() >= 2, "expected at least 2 links");
        let svg_pos = links.iter().position(|l| l.ends_with(".svg"));
        let raster_pos = links.iter().position(|l| !l.ends_with(".svg"));
        assert!(svg_pos.is_some(), "svg must be present");
        assert!(raster_pos.is_some(), "raster must be present");
        assert!(raster_pos.unwrap() < svg_pos.unwrap(), "raster must come before svg");
    }

    #[test]
    fn find_icon_links_resolves_relative_urls() {
        let html = r#"<link rel="icon" href="/static/favicon.ico">"#;
        let links = find_icon_links(html, "https://example.com/page");
        assert_eq!(links, vec!["https://example.com/static/favicon.ico"]);
    }

    #[test]
    fn dedup_by_domain_groups_same_domain() {
        let nodes = vec![
            make_node(1, "https://github.com/foo"),
            make_node(2, "https://github.com/bar"),
            make_node(3, "https://example.com/"),
        ];
        let deduped = dedup_by_domain(nodes);
        assert_eq!(deduped.len(), 2, "two unique domains");
        let gh = deduped
            .iter()
            .find(|(n, _)| n.url.as_deref().unwrap_or("").contains("github"))
            .unwrap();
        assert_eq!(gh.1.len(), 2, "both github IDs grouped");
        assert!(gh.1.contains(&1) && gh.1.contains(&2));
    }

    #[test]
    fn dedup_by_domain_unique_domains_stay_separate() {
        let nodes = vec![
            make_node(1, "https://a.com/"),
            make_node(2, "https://b.com/"),
            make_node(3, "https://c.com/"),
        ];
        let deduped = dedup_by_domain(nodes);
        assert_eq!(deduped.len(), 3);
        for (_, ids) in &deduped {
            assert_eq!(ids.len(), 1);
        }
    }
}
