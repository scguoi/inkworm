use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::clock::Clock;
use crate::config::Config;
use crate::storage::course::Course;
use crate::storage::progress::Progress;
use crate::storage::DataPaths;
use crate::ui::error_banner::user_message;
use crate::ui::generate::{GenerateSubstate, PastingState, RunningState, ResultState};
use crate::ui::palette::{Command, PaletteState};
use crate::ui::study::{FeedbackState, StudyState};
use crate::ui::task_msg::{GenerateProgress, TaskMsg};

pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,
    DeleteConfirm,
}

pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub study: StudyState,
    pub palette: Option<PaletteState>,
    pub data_paths: DataPaths,
    pub clock: Arc<dyn Clock>,
    blink_counter: u32,
    pub cursor_visible: bool,
    pub task_tx: mpsc::Sender<TaskMsg>,
    pub generate: Option<GenerateSubstate>,
    pub config: Config,
    pub delete_confirming: Option<String>,
}

impl App {
    pub fn new(
        course: Option<Course>,
        progress: Progress,
        data_paths: DataPaths,
        clock: Arc<dyn Clock>,
        config: Config,
        task_tx: mpsc::Sender<TaskMsg>,
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
            task_tx,
            generate: None,
            config,
            delete_confirming: None,
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
        match event {
            Event::Key(key) => match &self.screen {
                Screen::Study => self.handle_study_key(key),
                Screen::Palette => self.handle_palette_key(key),
                Screen::Help => self.handle_help_key(key),
                Screen::Generate => self.handle_generate_key(key),
                Screen::DeleteConfirm => self.handle_delete_confirm_key(key),
            },
            Event::Paste(text) => {
                if let Screen::Generate = self.screen {
                    if let Some(GenerateSubstate::Pasting(ref mut p)) = self.generate {
                        p.text.push_str(&text);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn on_task_msg(&mut self, msg: TaskMsg) {
        match msg {
            TaskMsg::Generate(progress) => self.handle_generate_progress(progress),
            TaskMsg::Wizard(_) => {} // placeholder — wired up in Task 5
        }
    }

    fn handle_generate_progress(&mut self, progress: GenerateProgress) {
        match progress {
            GenerateProgress::Phase1Started => {
                if let Some(GenerateSubstate::Running(ref mut state)) = self.generate {
                    state.phase_label = "Splitting article into sentences…".to_string();
                }
            }
            GenerateProgress::Phase1Done { sentence_count } => {
                if let Some(GenerateSubstate::Running(ref mut state)) = self.generate {
                    state.phase_label = "Generating drills…".to_string();
                    state.total = sentence_count;
                }
            }
            GenerateProgress::Phase2Progress { done, total } => {
                if let Some(GenerateSubstate::Running(ref mut state)) = self.generate {
                    state.done = done;
                    state.total = total;
                    state.phase_label = format!("Generating drills: {done}/{total}");
                }
            }
            GenerateProgress::Done(course) => {
                let course_id = course.id.clone();
                if let Err(e) =
                    crate::storage::course::save_course(&self.data_paths.courses_dir, &course)
                {
                    let article_text = String::new();
                    self.generate = Some(GenerateSubstate::Result(ResultState::failure(
                        user_message(&crate::error::AppError::Storage(e)),
                        article_text,
                    )));
                    return;
                }
                self.study.progress_mut().active_course_id = Some(course_id.clone());
                let _ = self.study.progress().save(&self.data_paths.progress_file);
                self.study = StudyState::new(Some(course), self.study.progress().clone());
                self.generate = None;
                self.screen = Screen::Study;
            }
            GenerateProgress::Failed(err) => {
                let article_text = String::new();
                self.generate = Some(GenerateSubstate::Result(ResultState::failure(
                    user_message(&err),
                    article_text,
                )));
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
                self.study.advance();
            }
            FeedbackState::Wrong { .. } => match key.code {
                KeyCode::Char(c) => self.study.type_char(c),
                KeyCode::Backspace => self.study.backspace(),
                KeyCode::Enter => self.study.submit(self.clock.as_ref()),
                _ => {}
            },
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

    fn handle_generate_key(&mut self, key: KeyEvent) {
        let Some(ref gen_state) = self.generate else {
            return;
        };

        match gen_state {
            GenerateSubstate::Pasting(_) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') => { self.quit(); return; }
                        KeyCode::Enter => {
                            let can_submit = if let Some(GenerateSubstate::Pasting(p)) = &self.generate {
                                p.can_submit(self.config.generation.max_article_bytes)
                            } else { false };
                            if can_submit {
                                let text = if let Some(GenerateSubstate::Pasting(p)) = &self.generate {
                                    p.text.clone()
                                } else { return };
                                self.submit_generate(text);
                            }
                            return;
                        }
                        _ => { return; }
                    }
                }
                match key.code {
                    KeyCode::Esc => {
                        self.generate = None;
                        self.screen = Screen::Study;
                    }
                    KeyCode::Char(c) => {
                        if let Some(GenerateSubstate::Pasting(ref mut p)) = self.generate {
                            p.type_char(c);
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(GenerateSubstate::Pasting(ref mut p)) = self.generate {
                            p.backspace();
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(GenerateSubstate::Pasting(ref mut p)) = self.generate {
                            p.type_char('\n');
                        }
                    }
                    _ => {}
                }
            }
            GenerateSubstate::Running(_) => {
                if key.code == KeyCode::Esc {
                    if let Some(GenerateSubstate::Running(ref r)) = self.generate {
                        r.cancel_token.cancel();
                    }
                    self.generate = Some(GenerateSubstate::Pasting(PastingState::new()));
                }
            }
            GenerateSubstate::Result(result) => {
                let success = result.success;
                let article_text = result.article_text.clone();
                match key.code {
                    KeyCode::Char('r') if !success => {
                        self.submit_generate(article_text);
                    }
                    KeyCode::Esc => {
                        if success {
                            self.generate = None;
                            self.screen = Screen::Study;
                        } else {
                            let mut pasting = PastingState::new();
                            pasting.text = article_text;
                            self.generate = Some(GenerateSubstate::Pasting(pasting));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_delete_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                if let Some(_title) = &self.delete_confirming {
                    if let Some(course) = self.study.current_course() {
                        let course_id = course.id.clone();
                        if let Err(e) = crate::storage::course::delete_course(
                            &self.data_paths.courses_dir,
                            &course_id,
                        ) {
                            eprintln!("Failed to delete course: {e}");
                        }
                        self.study.progress_mut().courses.remove(&course_id);
                        self.study.progress_mut().active_course_id = None;
                        let _ = self.study.progress().save(&self.data_paths.progress_file);
                        self.study = StudyState::new(None, self.study.progress().clone());
                    }
                }
                self.delete_confirming = None;
                self.screen = Screen::Study;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.delete_confirming = None;
                self.screen = Screen::Study;
            }
            _ => {}
        }
    }

    fn submit_generate(&mut self, article: String) {
        let running = RunningState::new();
        let cancel_token = running.cancel_token.clone();
        self.generate = Some(GenerateSubstate::Running(running));

        let task_tx = self.task_tx.clone();
        let config = self.config.clone();
        let data_paths = self.data_paths.clone();
        let clock = self.clock.clone();
        let existing_ids: Vec<String> = self.study.progress().courses.keys().cloned().collect();

        tokio::spawn(async move {
            let client = match crate::llm::client::ReqwestClient::new(
                config.llm.base_url.clone(),
                config.llm.api_key.clone(),
                Duration::from_secs(config.llm.request_timeout_secs),
            ) {
                Ok(c) => c,
                Err(e) => {
                    let _ = task_tx
                        .send(TaskMsg::Generate(GenerateProgress::Failed(
                            crate::error::AppError::Llm(e),
                        )))
                        .await;
                    return;
                }
            };

            let reflexion = crate::llm::reflexion::Reflexion {
                client: &client,
                clock: clock.as_ref(),
                paths: &data_paths,
                model: &config.llm.model,
                max_concurrent: config.generation.max_concurrent_calls,
                cancel: cancel_token,
            };

            let (progress_tx, mut progress_rx) = mpsc::channel(16);
            let progress_tx_clone = progress_tx.clone();
            let task_tx_forwarder = task_tx.clone();

            tokio::spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    let _ = task_tx_forwarder.send(TaskMsg::Generate(progress)).await;
                }
            });

            match reflexion
                .generate(&article, &existing_ids, Some(progress_tx_clone))
                .await
            {
                Ok(outcome) => {
                    let _ = task_tx
                        .send(TaskMsg::Generate(GenerateProgress::Done(outcome.course)))
                        .await;
                }
                Err(e) => {
                    let app_err = match e {
                        crate::llm::reflexion::ReflexionError::Llm(llm_err) => {
                            crate::error::AppError::Llm(llm_err)
                        }
                        crate::llm::reflexion::ReflexionError::Cancelled => {
                            crate::error::AppError::Cancelled
                        }
                        crate::llm::reflexion::ReflexionError::AllAttemptsFailed {
                            saved_to, ..
                        } => crate::error::AppError::Reflexion {
                            attempts: 3,
                            saved_to,
                            summary: "Generation failed".to_string(),
                        },
                        crate::llm::reflexion::ReflexionError::Storage(s) => {
                            crate::error::AppError::Storage(s)
                        }
                        crate::llm::reflexion::ReflexionError::BudgetExceeded => {
                            crate::error::AppError::Reflexion {
                                attempts: 0,
                                saved_to: std::path::PathBuf::new(),
                                summary: "Budget exceeded".to_string(),
                            }
                        }
                    };
                    let _ = task_tx
                        .send(TaskMsg::Generate(GenerateProgress::Failed(app_err)))
                        .await;
                }
            }
        });
    }

    fn execute_command(&mut self, cmd: &Command) {
        match cmd.name {
            "quit" | "q" => self.quit(),
            "skip" => self.study.skip(),
            "help" => self.screen = Screen::Help,
            "import" => {
                self.generate = Some(GenerateSubstate::Pasting(PastingState::new()));
                self.screen = Screen::Generate;
            }
            "delete" => {
                if let Some(course) = self.study.current_course() {
                    self.delete_confirming = Some(course.title.clone());
                    self.screen = Screen::DeleteConfirm;
                }
            }
            _ => {}
        }
    }

    fn quit(&mut self) {
        let _ = self.study.progress().save(&self.data_paths.progress_file);
        self.should_quit = true;
    }

    pub fn render(&self, frame: &mut Frame) {
        match &self.screen {
            Screen::Study => {
                crate::ui::study::render_study(frame, &self.study, self.cursor_visible)
            }
            Screen::Palette => {
                crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
                if let Some(palette) = &self.palette {
                    crate::ui::palette::render_palette(frame, palette);
                }
            }
            Screen::Help => crate::ui::palette::render_help(frame),
            Screen::Generate => {
                if let Some(ref gen_state) = self.generate {
                    crate::ui::generate::render_generate(
                        frame,
                        gen_state,
                        self.config.generation.max_article_bytes,
                    );
                }
            }
            Screen::DeleteConfirm => {
                crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
                if let Some(ref title) = self.delete_confirming {
                    render_delete_confirm(frame, title);
                }
            }
        }
    }
}

fn render_delete_confirm(frame: &mut Frame, title: &str) {
    use ratatui::{
        layout::Rect,
        style::{Color, Style},
        text::Span,
        widgets::Paragraph,
    };

    let area = frame.area();
    let y = area.height / 2;
    let msg = format!("Delete course \"{}\"? (y/n)", title);
    let para = Paragraph::new(Span::styled(msg, Style::default().fg(Color::Yellow))).centered();
    frame.render_widget(para, Rect::new(0, y, area.width, 1));
}
