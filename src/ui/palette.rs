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
    Command { name: "import", aliases: &[], description: "Create a new course", available: true },
    Command { name: "list", aliases: &[], description: "Browse courses", available: false },
    Command { name: "config", aliases: &[], description: "Configuration wizard", available: true },
    Command { name: "tts", aliases: &[], description: "TTS settings", available: false },
    Command { name: "delete", aliases: &[], description: "Delete current course", available: true },
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

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, Paragraph},
};

pub fn render_palette(frame: &mut Frame, state: &PaletteState) {
    let area = frame.area();
    let matches = state.matches();

    let list_height = (matches.len() as u16).min(10).min(area.height.saturating_sub(3));
    let total_height = list_height + 1;
    let y = area.height.saturating_sub(total_height);
    let width = 40u16.min(area.width);
    let x = (area.width.saturating_sub(width)) / 2;

    let palette_rect = Rect::new(x, y, width, total_height);
    frame.render_widget(Clear, palette_rect);

    if !matches.is_empty() {
        let items: Vec<ListItem> = matches
            .iter()
            .enumerate()
            .map(|(i, cmd)| {
                let style = if i == state.selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if cmd.available {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let suffix = if !cmd.available { " (coming soon)" } else { "" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("/{}", cmd.name), style),
                    Span::styled(format!("  {}{}", cmd.description, suffix), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let list = List::new(items);
        frame.render_widget(list, Rect::new(x, y, width, list_height));
    }

    let input_line = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::DarkGray)),
        Span::styled(state.input.clone(), Style::default().fg(Color::White)),
    ]));
    frame.render_widget(input_line, Rect::new(x, y + list_height, width, 1));
}

pub fn render_help(frame: &mut Frame) {
    let area = frame.area();
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled("Commands", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(""),
    ];
    for cmd in COMMANDS {
        let status = if cmd.available { "" } else { " (coming soon)" };
        let aliases = if cmd.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", cmd.aliases.iter().map(|a| format!("/{a}")).collect::<Vec<_>>().join(", "))
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  /{}{}", cmd.name, aliases), Style::default().fg(Color::White)),
            Span::styled(format!("  {}{}", cmd.description, status), Style::default().fg(Color::DarkGray)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Press any key to close", Style::default().fg(Color::DarkGray))));

    let height = lines.len() as u16;
    let y = area.height.saturating_sub(height) / 2;
    let para = Paragraph::new(lines).centered();
    frame.render_widget(para, Rect::new(0, y, area.width, height));
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
