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

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

#[derive(Debug, Clone)]
pub struct ShellHeader {
    user: String,
    host: String,
    cwd: String,
}

impl ShellHeader {
    /// Capture user/host/cwd from the environment. Called once at app start.
    pub fn detect() -> Self {
        let user = whoami::username();
        let host = whoami::fallible::hostname().unwrap_or_else(|_| "localhost".to_string());
        let cwd_raw = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "?".to_string());
        let home = std::env::var("HOME").ok();
        let cwd = home_rewrite(&cwd_raw, home.as_deref());
        Self { user, host, cwd }
    }

    /// Build a Line that fits within `width` columns.
    pub fn render(&self, width: u16) -> Line<'static> {
        let prefix = format!("{}@{} ", self.user, self.host);
        let suffix = " $ ";
        let prefix_len = prefix.chars().count();
        let suffix_len = suffix.chars().count();
        let width = width as usize;

        let cwd_disp = if prefix_len + self.cwd.chars().count() + suffix_len <= width {
            self.cwd.clone()
        } else {
            let cwd_budget = width.saturating_sub(prefix_len + suffix_len);
            truncate_cwd(&self.cwd, cwd_budget)
        };

        let style = Style::default().fg(Color::DarkGray);
        Line::from(vec![Span::styled(
            format!("{}{}{}", prefix, cwd_disp, suffix),
            style,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_rewrite_replaces_home_prefix() {
        assert_eq!(
            home_rewrite("/Users/scguo/.tries/x", Some("/Users/scguo")),
            "~/.tries/x"
        );
    }

    #[test]
    fn home_rewrite_keeps_path_when_outside_home() {
        assert_eq!(
            home_rewrite("/etc/passwd", Some("/Users/scguo")),
            "/etc/passwd"
        );
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

    use ratatui::style::Color;

    fn header_fixture() -> ShellHeader {
        ShellHeader {
            user: "scguo".to_string(),
            host: "MacBook-Pro".to_string(),
            cwd: "~/.tries/2026-04-21-scguoi/inkworm".to_string(),
        }
    }

    #[test]
    fn header_renders_full_when_width_is_ample() {
        let h = header_fixture();
        let line = h.render(200);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            text,
            "scguo@MacBook-Pro ~/.tries/2026-04-21-scguoi/inkworm $ "
        );
    }

    #[test]
    fn header_truncates_cwd_when_narrow() {
        let h = header_fixture();
        // user@host = "scguo@MacBook-Pro " (18). suffix = "$ " (2).
        // width 40 → cwd budget = 40 - 18 - 2 = 20.
        let line = h.render(40);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("scguo@MacBook-Pro "));
        assert!(text.ends_with("$ "));
        assert!(text.chars().count() <= 40);
        assert!(text.contains("…/inkworm"));
    }

    #[test]
    fn header_uses_dark_gray() {
        let h = header_fixture();
        let line = h.render(200);
        for span in &line.spans {
            assert_eq!(span.style.fg, Some(Color::DarkGray));
        }
    }

    #[test]
    fn header_extreme_narrow_does_not_panic() {
        let h = header_fixture();
        let _ = h.render(5);
        let _ = h.render(0);
    }
}
