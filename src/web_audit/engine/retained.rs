//! Derivations from the wave-1 bodies the engine keeps for wave 2: the
//! section directories the scoped llms.txt probes enumerate, from the root
//! llms.txt link index unioned with the sitemap's paths.

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

static MARKDOWN_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\]\(([^)\s]+)\)").unwrap());
static SITEMAP_LOC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<loc>\s*([^<\s]+)\s*</loc>").unwrap());

/// The top-level section directory of a same-origin href with at least two
/// path segments, or `None`. An off-origin href is dropped, never fetched.
fn top_level_dir(href: &str, base: &Url) -> Option<String> {
    let url = base.join(href).ok()?;
    if url.origin() != base.origin() {
        return None;
    }
    let segments: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    Some(format!("/{}", segments[0]))
}

/// The deduplicated union of top-level section directories referenced by
/// the root llms.txt link index and the sitemap, in encounter order.
pub fn enumerate_scoped_dirs(llms_body: &str, sitemap_body: &str, base: &str) -> Vec<String> {
    let Ok(base) = Url::parse(base) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = Vec::new();
    let mut push = |dir: Option<String>| {
        if let Some(dir) = dir
            && !dirs.contains(&dir)
        {
            dirs.push(dir);
        }
    };
    for m in MARKDOWN_LINK_RE.captures_iter(llms_body) {
        push(top_level_dir(&m[1], &base));
    }
    for m in SITEMAP_LOC_RE.captures_iter(sitemap_body) {
        push(top_level_dir(&m[1], &base));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_dirs_come_from_both_indexes_same_origin_only_and_deduplicated() {
        let llms = "# X\n- [Guide](/docs/guide.md)\n- [API](https://example.com/api/v1)\n- [Root](/llms-full.txt)\n- [Other](https://other.test/docs/x)\n- [Private](http://10.0.0.1/docs/x)\n";
        let sitemap = "<urlset><url><loc> https://example.com/blog/post </loc></url><url><LOC>https://example.com/docs/other</LOC></url></urlset>";
        assert_eq!(
            enumerate_scoped_dirs(llms, sitemap, "https://example.com/"),
            ["/docs", "/api", "/blog"]
        );
        assert!(enumerate_scoped_dirs(llms, sitemap, "not a url").is_empty());
        assert!(enumerate_scoped_dirs("", "", "https://example.com/").is_empty());
    }
}
