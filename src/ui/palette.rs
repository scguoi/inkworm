#[derive(Debug, Clone)]
pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub available: bool,
}

pub const COMMANDS: &[Command] = &[
    Command { name: "quit", aliases: &["q"], description: "Save progress and exit", available: true },
    Command { name: "skip", aliases: &[], description: "Skip current drill", available: true },
    Command { name: "help", aliases: &[], description: "Show command list", available: true },
    Command { name: "import", aliases: &[], description: "Create a new course", available: false },
    Command { name: "list", aliases: &[], description: "Browse courses", available: false },
    Command { name: "config", aliases: &[], description: "Configuration wizard", available: false },
    Command { name: "tts", aliases: &[], description: "TTS settings", available: false },
    Command { name: "delete", aliases: &[], description: "Delete current course", available: false },
    Command { name: "logs", aliases: &[], description: "Show log file path", available: false },
    Command { name: "doctor", aliases: &[], description: "Health check", available: false },
];

pub struct PaletteState {
    pub input: String,
    pub selected: usize,
}

impl PaletteState {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            selected: 0,
        }
    }

    pub fn matches(&self) -> Vec<&'static Command> {
        let query = self.input.trim_start_matches('/').to_lowercase();
        if query.is_empty() {
            return COMMANDS.iter().collect();
        }
        COMMANDS
            .iter()
            .filter(|cmd| {
                cmd.name.starts_with(&query)
                    || cmd.aliases.iter().any(|a| a.starts_with(&query))
            })
            .collect()
    }

    pub fn type_char(&mut self, c: char) {
        self.input.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.input.pop();
        self.selected = 0;
    }

    pub fn select_next(&mut self) {
        let count = self.matches().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    pub fn select_prev(&mut self) {
        let count = self.matches().len();
        if count > 0 {
            self.selected = (self.selected + count - 1) % count;
        }
    }

    pub fn complete(&mut self) {
        let matches = self.matches();
        if let Some(cmd) = matches.get(self.selected) {
            self.input = format!("/{}", cmd.name);
        }
    }

    pub fn confirm(&self) -> Option<&'static Command> {
        let matches = self.matches();
        matches.get(self.selected).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_all() {
        let p = PaletteState::new();
        assert_eq!(p.matches().len(), COMMANDS.len());
    }

    #[test]
    fn prefix_filters() {
        let mut p = PaletteState::new();
        p.type_char('q');
        let m = p.matches();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "quit");
    }

    #[test]
    fn slash_prefix_ignored() {
        let mut p = PaletteState::new();
        for c in "/sk".chars() {
            p.type_char(c);
        }
        let m = p.matches();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "skip");
    }

    #[test]
    fn alias_match() {
        let mut p = PaletteState::new();
        p.type_char('q');
        let m = p.matches();
        assert!(m.iter().any(|c| c.name == "quit"));
    }

    #[test]
    fn tab_completes() {
        let mut p = PaletteState::new();
        p.type_char('h');
        p.complete();
        assert_eq!(p.input, "/help");
    }

    #[test]
    fn confirm_returns_selected() {
        let mut p = PaletteState::new();
        for c in "quit".chars() {
            p.type_char(c);
        }
        let cmd = p.confirm().unwrap();
        assert_eq!(cmd.name, "quit");
    }

    #[test]
    fn no_match_returns_empty() {
        let mut p = PaletteState::new();
        for c in "zzz".chars() {
            p.type_char(c);
        }
        assert!(p.matches().is_empty());
        assert!(p.confirm().is_none());
    }
}
