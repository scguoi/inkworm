use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;

use crate::clock::Clock;
use crate::storage::course::Course;
use crate::storage::progress::Progress;
use crate::storage::DataPaths;
use crate::ui::palette::{PaletteState, Command};
use crate::ui::study::{FeedbackState, StudyState};

pub enum Screen {
    Study,
    Palette,
    Help,
}

pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub study: StudyState,
    pub palette: Option<PaletteState>,
    pub data_paths: DataPaths,
    pub clock: Box<dyn Clock>,
    blink_counter: u32,
    pub cursor_visible: bool,
}

impl App {
    pub fn new(
        course: Option<Course>,
        progress: Progress,
        data_paths: DataPaths,
        clock: Box<dyn Clock>,
    ) -> Self {
        Self {
            screen: Screen::Study,
            should_quit: false,
            study: StudyState::new(course, progress),
            palette: None,
            data_paths,
            clock,
            blink_counter: 0,
            cursor_visible: true,
        }
    }

    pub fn on_tick(&mut self) {
        self.blink_counter += 1;
        if self.blink_counter >= 33 {
            self.blink_counter = 0;
            self.cursor_visible = !self.cursor_visible;
        }
    }

    pub fn on_input(&mut self, event: Event) {
        if let Event::Key(key) = event {
            match &self.screen {
                Screen::Study => self.handle_study_key(key),
                Screen::Palette => self.handle_palette_key(key),
                Screen::Help => self.handle_help_key(key),
            }
        }
    }

    fn handle_study_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('p') => {
                    self.palette = Some(PaletteState::new());
                    self.screen = Screen::Palette;
                }
                KeyCode::Char('c') => self.quit(),
                _ => {}
            }
            return;
        }
        match self.study.feedback() {
            FeedbackState::Correct => {
                // Any key advances, but is not consumed as input
                self.study.advance();
            }
            FeedbackState::Wrong { .. } => {
                // Allow editing: type, backspace, enter to re-submit
                match key.code {
                    KeyCode::Char(c) => self.study.type_char(c),
                    KeyCode::Backspace => self.study.backspace(),
                    KeyCode::Enter => self.study.submit(self.clock.as_ref()),
                    _ => {}
                }
            }
            FeedbackState::Typing => match key.code {
                KeyCode::Char(c) => self.study.type_char(c),
                KeyCode::Backspace => self.study.backspace(),
                KeyCode::Enter => self.study.submit(self.clock.as_ref()),
                KeyCode::Tab => self.study.skip(),
                _ => {}
            },
        }
    }

    fn handle_palette_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.palette = None;
                self.screen = Screen::Study;
            }
            KeyCode::Char(c) => {
                if let Some(p) = &mut self.palette {
                    p.type_char(c);
                }
            }
            KeyCode::Backspace => {
                if let Some(p) = &mut self.palette {
                    p.backspace();
                    if p.input.is_empty() {
                        self.palette = None;
                        self.screen = Screen::Study;
                    }
                }
            }
            KeyCode::Tab => {
                if let Some(p) = &mut self.palette {
                    p.complete();
                }
            }
            KeyCode::Up => {
                if let Some(p) = &mut self.palette {
                    p.select_prev();
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.palette {
                    p.select_next();
                }
            }
            KeyCode::Enter => {
                if let Some(p) = &self.palette {
                    if let Some(cmd) = p.confirm() {
                        self.execute_command(cmd);
                    }
                }
                if !self.should_quit {
                    self.palette = None;
                    self.screen = Screen::Study;
                }
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, _key: KeyEvent) {
        self.screen = Screen::Study;
    }

    fn execute_command(&mut self, cmd: &Command) {
        match cmd.name {
            "quit" | "q" => self.quit(),
            "skip" => self.study.skip(),
            "help" => self.screen = Screen::Help,
            _ => {
                // "coming soon" — handled in render
            }
        }
    }

    fn quit(&mut self) {
        let _ = self.study.progress().save(&self.data_paths.progress_file);
        self.should_quit = true;
    }

    pub fn render(&self, frame: &mut Frame) {
        match &self.screen {
            Screen::Study => crate::ui::study::render_study(frame, &self.study, self.cursor_visible),
            Screen::Palette => {
                crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
                if let Some(palette) = &self.palette {
                    crate::ui::palette::render_palette(frame, palette);
                }
            }
            Screen::Help => crate::ui::palette::render_help(frame),
        }
    }
}
