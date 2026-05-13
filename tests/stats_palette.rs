use inkworm::ui::palette::{PaletteState, COMMANDS};

#[test]
fn stats_is_a_registered_command() {
    assert!(COMMANDS.iter().any(|c| c.name == "stats"));
}

#[test]
fn slash_stats_parses_to_stats_command() {
    let mut p = PaletteState::new();
    p.input = "/stats".into();
    let (cmd, args) = p.parse();
    assert_eq!(cmd, "stats");
    assert!(args.is_empty());
}
