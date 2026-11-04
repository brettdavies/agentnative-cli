//! The name lists bare routing reads: the root zone, the file extensions, and the
//! private-use suffixes.

/// The root zone's top-level domains, one per line, from IANA.
/// `scripts/sync-tlds.sh` refreshes it; its first line is IANA's version stamp.
const ROOT_ZONE_TLDS: &str = include_str!("tlds-alpha-by-domain.txt");

/// GitHub Linguist's file extensions, one last label per line, lowercased.
/// `scripts/sync-linguist-extensions.sh` refreshes it; its first line names the commit.
const LINGUIST_EXTENSIONS: &str = include_str!("linguist-extensions.txt");

/// Extensions that read as a filename and that no language in Linguist claims. Some
/// are top-level domains too (`.zip`, `.mov`, `.so`, `.la`, `.tt`), and `.ac` and `.am`
/// are autotools inputs (`configure.ac`, `Makefile.am`).
const OTHER_EXTENSIONS: &[&str] = &[
    "ac", "am", "bak", "bin", "conf", "deb", "dmg", "exe", "git", "gz", "heic", "jar", "jpeg",
    "jpg", "la", "lock", "log", "mov", "o", "out", "pdf", "pem", "png", "rpm", "so", "tar", "tgz",
    "tmp", "tt", "wasm", "zip",
];

/// Top-level domains in Linguist's list where real sites plainly outnumber files anyone
/// would type, so a bare name ending in one still routes. Linguist at c26d14a50f63
/// has `.com` for DIGITAL Command Language, `.org` for Org mode, `.io` for Io, and `.fr`
/// for Frege. Every other top-level domain it lists stays offline, where the error names
/// `anc web <target>`.
const SITES_OUTNUMBER_FILES: &[&str] = &[
    "be", "ch", "cl", "com", "es", "fr", "il", "io", "me", "nl", "no", "org", "ph", "pt", "ru",
    "ua",
];

/// Suffixes that only name private hosts, so a port on one reads as a host: ICANN reserved
/// `.internal` (Board resolution 2024.07.29.06), RFC 8375 defines `home.arpa`, RFC 6762
/// defines `.local`, and its Appendix G records `.lan` in common private use.
const PRIVATE_USE_SUFFIXES: &[&str] = &["internal", "home.arpa", "local", "lan"];

/// Whether `label` is a top-level domain in the root zone. The list's comment line can
/// never match, because a caller passes a label with no spaces.
pub(in crate::web_audit) fn is_root_zone_tld(label: &str) -> bool {
    ROOT_ZONE_TLDS
        .lines()
        .any(|tld| tld.eq_ignore_ascii_case(label))
}

/// Whether a bare name's last label reads as a file extension.
pub(in crate::web_audit) fn reads_as_file_extension(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    let label = label.as_str();
    !SITES_OUTNUMBER_FILES.contains(&label)
        && (OTHER_EXTENSIONS.contains(&label)
            || LINGUIST_EXTENSIONS.lines().any(|ext| ext == label))
}

/// Whether `host` is a name under a private-use suffix, rather than the suffix itself.
pub(in crate::web_audit) fn is_under_private_use_suffix(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    PRIVATE_USE_SUFFIXES.iter().any(|suffix| {
        host.strip_suffix(suffix)
            .is_some_and(|name| name.len() > 1 && name.ends_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::target::looks_like_url;

    fn linguist() -> Vec<&'static str> {
        LINGUIST_EXTENSIONS.lines().skip(1).collect()
    }

    #[test]
    fn every_linguist_extension_that_is_a_top_level_domain_is_denied_except_this_list() {
        let mut denied = 0;
        for ext in linguist() {
            if is_root_zone_tld(ext) && !SITES_OUTNUMBER_FILES.contains(&ext) {
                denied += 1;
                for token in [format!("x.{ext}"), format!("x.{ext}:12")] {
                    assert!(!looks_like_url(&token), "{token}");
                }
            }
        }
        assert!(denied >= 90, "{denied} denied");
    }

    #[test]
    fn an_exempt_domain_is_on_both_lists_and_routes() {
        for tld in SITES_OUTNUMBER_FILES {
            assert!(linguist().contains(tld), "{tld} is not in Linguist's list");
            assert!(is_root_zone_tld(tld), "{tld} is not a top-level domain");
            for token in [format!("x.{tld}"), format!("x.{tld}:8080")] {
                assert!(looks_like_url(&token), "{token}");
            }
        }
    }

    #[test]
    fn the_other_extensions_add_to_linguists_list() {
        for ext in OTHER_EXTENSIONS {
            assert!(!linguist().contains(ext), "{ext} is in Linguist's list");
        }
    }

    #[test]
    fn a_private_use_suffix_takes_a_name_in_front_of_it() {
        for host in [
            "a.internal",
            "A.INTERNAL",
            "nas.home.arpa",
            "p.local",
            "box.lan",
        ] {
            assert!(is_under_private_use_suffix(host), "{host}");
        }
        for host in [
            "internal",
            ".internal",
            "home.arpa",
            "xinternal",
            "a.internal.dev",
        ] {
            assert!(!is_under_private_use_suffix(host), "{host}");
        }
    }

    #[test]
    fn the_vendored_lists_read_as_their_refresh_scripts_require() {
        let mut tlds = ROOT_ZONE_TLDS.lines();
        let stamp = tlds.next().unwrap_or_default();
        assert!(stamp.starts_with("# Version "), "{stamp}");
        let mut names: Vec<&str> = tlds.collect();
        assert!(names.len() >= 1000, "{} names", names.len());
        for name in &names {
            assert!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-'),
                "{name:?}"
            );
        }
        for tld in ["COM", "DEV", "FR", "XN--P1AI"] {
            assert!(names.contains(&tld), "{tld}");
        }
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "a name repeats");

        let stamp = LINGUIST_EXTENSIONS.lines().next().unwrap_or_default();
        assert!(stamp.starts_with("# github-linguist/linguist@"), "{stamp}");
        let names = linguist();
        assert!(names.len() >= 1000, "{} extensions", names.len());
        for name in &names {
            assert!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{name:?}"
            );
        }
        assert!(names.is_sorted_by(|a, b| a < b), "not sorted and unique");
        for ext in ["rs", "go", "py", "nix", "in"] {
            assert!(names.contains(&ext), "{ext}");
        }
    }
}
