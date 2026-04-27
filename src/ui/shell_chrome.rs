//! Shell-style chrome around the study screen: top prompt header and
//! bottom reverse-video status bar.

fn home_rewrite(cwd: &str, home: Option<&str>) -> String {
    let Some(home) = home else {
        return cwd.to_string();
    };
    if cwd == home {
        return "~".to_string();
    }
    if let Some(rest) = cwd.strip_prefix(home).and_then(|r| r.strip_prefix('/')) {
        return format!("~/{}", rest);
    }
    cwd.to_string()
}

/// Shorten `cwd` to fit `max` chars by eliding the middle, keeping the
/// last path segment intact. If even `…/{last}` doesn't fit, clip the
/// raw path. `max == 0` returns empty string.
fn truncate_cwd(cwd: &str, max: usize) -> String {
    if cwd.chars().count() <= max {
        return cwd.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let last = cwd.rsplit('/').next().unwrap_or("");
    let last_len = last.chars().count();
    let ellipsis = "…/";
    let ellipsis_len = ellipsis.chars().count(); // 2

    if last_len + ellipsis_len >= max {
        // Can't even fit "…/{last}". Clip raw input.
        return cwd.chars().take(max).collect();
    }

    let head_budget = max - last_len - ellipsis_len;
    let head: String = cwd.chars().take(head_budget).collect();
    format!("{}{}{}", head, ellipsis, last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_rewrite_replaces_home_prefix() {
        assert_eq!(home_rewrite("/Users/scguo/.tries/x", Some("/Users/scguo")), "~/.tries/x");
    }

    #[test]
    fn home_rewrite_keeps_path_when_outside_home() {
        assert_eq!(home_rewrite("/etc/passwd", Some("/Users/scguo")), "/etc/passwd");
    }

    #[test]
    fn home_rewrite_keeps_path_when_home_unset() {
        assert_eq!(home_rewrite("/Users/scguo/x", None), "/Users/scguo/x");
    }

    #[test]
    fn home_rewrite_handles_exact_home() {
        assert_eq!(home_rewrite("/Users/scguo", Some("/Users/scguo")), "~");
    }

    #[test]
    fn truncate_cwd_returns_unchanged_when_fits() {
        assert_eq!(truncate_cwd("~/a/b", 10), "~/a/b");
    }

    #[test]
    fn truncate_cwd_elides_middle_keeping_last_segment() {
        // Input length 34, max 24. Keep last segment "inkworm" (7) and "…/" (2).
        // Head budget = 24 - 7 - 2 = 15. Head = first 15 chars of cwd.
        let out = truncate_cwd("~/.tries/2026-04-21-scguoi/inkworm", 24);
        assert_eq!(out, "~/.tries/2026-0…/inkworm");
        assert_eq!(out.chars().count(), 24);
    }

    #[test]
    fn truncate_cwd_when_last_alone_too_long_returns_clipped() {
        // Last segment is "very-long-name" (14). max=10. Can't fit "…/" + last.
        // Fall back to clipping the path to max chars.
        let out = truncate_cwd("/a/b/very-long-name", 10);
        assert_eq!(out.chars().count(), 10);
    }

    #[test]
    fn truncate_cwd_root_path() {
        assert_eq!(truncate_cwd("/", 5), "/");
    }
}
