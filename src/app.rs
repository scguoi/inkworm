use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::clock::Clock;
use crate::config::Config;
use crate::storage::course::Course;
use crate::storage::mistakes::MistakeBook;
use crate::storage::progress::Progress;
use crate::storage::DataPaths;
use crate::tts::speaker::Speaker;
use crate::tts::{should_play_bundle, OutputKind};
use crate::ui::error_banner::user_message;
use crate::ui::generate::{GenerateSubstate, PastingState, ResultState, RunningState};
use crate::ui::palette::{Command, PaletteState};
use crate::ui::study::{FeedbackState, StudyMode, StudyState};
use crate::ui::task_msg::{GenerateProgress, TaskMsg};

const MISTAKES_DONE_BANNER: &str = "Review complete for today ✓";

pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,
    DeleteConfirm,
    ConfigWizard,
    CourseList,
    TtsStatus,
    Doctor,
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
    pub mistakes: MistakeBook,
    pub delete_confirming: Option<String>,
    pub config_wizard: Option<crate::ui::config_wizard::WizardState>,
    pub course_list: Option<crate::ui::course_list::CourseListState>,
    pub speaker: Arc<dyn Speaker>,
    pub bundle_player: Arc<crate::audio::player::BundlePlayer>,
    pub current_device: OutputKind,
    device_probe_counter: u32,
    pub last_tts_error: Arc<tokio::sync::Mutex<Option<String>>>,
    pub tts_failure_count: u32,
    pub tts_session_disabled: bool,
    pub doctor_results: Option<Vec<crate::ui::doctor::CheckResult>>,
    pub info_banner: Option<String>,
    pub shell_header: crate::ui::shell_chrome::ShellHeader,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        course: Option<Course>,
        progress: Progress,
        data_paths: DataPaths,
        clock: Arc<dyn Clock>,
        config: Config,
        mistakes: MistakeBook,
        boot_warning: Option<String>,
        task_tx: mpsc::Sender<TaskMsg>,
        speaker: Arc<dyn Speaker>,
        bundle_player: Arc<crate::audio::player::BundlePlayer>,
    ) -> Self {
        let mut app = Self {
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
            mistakes,
            delete_confirming: None,
            config_wizard: None,
            course_list: None,
            speaker,
            bundle_player,
            current_device: OutputKind::Unknown,
            device_probe_counter: 0,
            last_tts_error: Arc::new(tokio::sync::Mutex::new(None)),
            tts_failure_count: 0,
            tts_session_disabled: false,
            doctor_results: None,
            info_banner: None,
            shell_header: crate::ui::shell_chrome::ShellHeader::detect(),
        };
        app.startup_apply_mistakes_session();
        if boot_warning.is_some() && app.info_banner.is_none() {
            app.info_banner = boot_warning;
        }
        app
    }

    fn startup_apply_mistakes_session(&mut self) {
        let today = self.clock.today_local();
        self.mistakes.ensure_session(today);
        if self.mistakes.peek_current_drill().is_some() {
            self.enter_mistakes_mode_at_current_drill();
        }
        self.save_mistakes();
    }

    fn load_course_owned(&self, id: &str) -> Option<crate::storage::course::Course> {
        crate::storage::course::load_course(&self.data_paths.courses_dir, id).ok()
    }

    /// Switches the study screen into Mistakes mode and points at the
    /// current session's drill. If the drill's course can't be loaded,
    /// purges that course's entries and recurses to the next drill.
    fn enter_mistakes_mode_at_current_drill(&mut self) {
        let Some(drill_ref) = self.mistakes.peek_current_drill() else {
            return;
        };
        let course = match self.load_course_owned(&drill_ref.course_id) {
            Some(c) => c,
            None => {
                self.mistakes.purge_course(&drill_ref.course_id);
                self.save_mistakes();
                if self.mistakes.peek_current_drill().is_some() {
                    self.enter_mistakes_mode_at_current_drill();
                }
                return;
            }
        };
        let Some(sentence_idx) = course
            .sentences
            .iter()
            .position(|s| s.order == drill_ref.sentence_order)
        else {
            self.purge_orphan_entry(&drill_ref);
            if self.mistakes.peek_current_drill().is_some() {
                self.enter_mistakes_mode_at_current_drill();
            }
            return;
        };
        let Some(drill_idx) = course.sentences[sentence_idx]
            .drills
            .iter()
            .position(|d| d.stage == drill_ref.drill_stage)
        else {
            self.purge_orphan_entry(&drill_ref);
            if self.mistakes.peek_current_drill().is_some() {
                self.enter_mistakes_mode_at_current_drill();
            }
            return;
        };
        let progress_clone = self.study.progress().clone();
        let mut new_state = StudyState::new(Some(course), progress_clone);
        new_state.set_mode(StudyMode::Mistakes);
        new_state.set_current_drill(sentence_idx, drill_idx);
        self.study = new_state;
        self.speak_current_drill();
    }

    fn save_mistakes(&mut self) {
        if let Err(e) = self.mistakes.save(&self.data_paths.mistakes_file) {
            let msg = format!("Failed to save review state: {e}");
            tracing::warn!("{msg}");
            self.info_banner = Some(msg);
        }
    }

    fn handle_submit_outcome(&mut self, outcome: crate::ui::study::SubmitOutcome) {
        match self.study.mode() {
            crate::ui::study::StudyMode::Course => {
                let _ = self.mistakes.record_normal_attempt(
                    &outcome.drill_ref,
                    outcome.first_attempt_correct,
                    self.clock.now(),
                );
            }
            crate::ui::study::StudyMode::Mistakes => {
                let round = self
                    .mistakes
                    .session_progress()
                    .map(|p| p.round)
                    .unwrap_or(1);
                let result = self.mistakes.record_mistakes_attempt(
                    &outcome.drill_ref,
                    round,
                    outcome.first_attempt_correct,
                    self.clock.today_local(),
                );
                if result.cleared {
                    self.info_banner = Some(format!(
                        "{} stage {} cleared ✓",
                        outcome.drill_ref.course_id, outcome.drill_ref.drill_stage
                    ));
                }
            }
        }
        self.save_mistakes();
    }

    /// Remove a single orphaned entry (course exists but sentence/stage no
    /// longer matches). Adjusts session.queue + next_index using the same
    /// logic as `purge_course`'s queue rebuild.
    fn purge_orphan_entry(&mut self, drill: &crate::storage::mistakes::DrillRef) {
        self.mistakes.entries.retain(|e| &e.drill != drill);
        if let Some(session) = self.mistakes.session.as_mut() {
            let shift_next = session.queue[..session.next_index]
                .iter()
                .filter(|d| *d == drill)
                .count();
            session.next_index -= shift_next;
            session.queue.retain(|d| d != drill);
            if session.queue.is_empty() {
                self.mistakes.session = None;
            }
        }
        self.save_mistakes();
    }

    fn enter_course_mode(&mut self) {
        let active_id = self.study.progress().active_course_id.clone();
        let course = active_id.and_then(|id| self.load_course_owned(&id));
        let progress = self.study.progress().clone();
        let mut new_state = StudyState::new(course, progress);
        new_state.set_mode(StudyMode::Course);
        self.study = new_state;
    }

    pub fn open_wizard(&mut self, origin: crate::ui::config_wizard::WizardOrigin) {
        use crate::ui::config_wizard::WizardState;
        let state = WizardState::new(origin, self.config.clone());
        self.config_wizard = Some(state);
        self.screen = Screen::ConfigWizard;
    }

    fn tts_has_creds(&self) -> bool {
        let cfg = &self.config.tts.iflytek;
        !cfg.app_id.trim().is_empty()
            && !cfg.api_key.trim().is_empty()
            && !cfg.api_secret.trim().is_empty()
    }

    /// Cancel any in-flight speak, then if there is a current drill,
    /// spawn a new speak for its English text. Safe to call on any state
    /// transition — no-ops cleanly when no drill is active.
    pub fn speak_current_drill(&self) {
        tracing::debug!("speak_current_drill called");
        self.speaker.cancel();
        self.bundle_player.cancel();

        // Don't speak if course is complete
        if *self.study.phase() == crate::ui::study::StudyPhase::Complete {
            tracing::debug!("Course complete, skipping TTS");
            return;
        }
        let Some(drill) = self.study.current_drill() else {
            tracing::debug!("No current drill, skipping");
            return;
        };

        // Device + mode gate applies uniformly to both bundle and TTS paths.
        // Creds are not checked here — the bundle path is purely local.
        let should_play = should_play_bundle(self.config.tts.r#override, self.current_device);
        tracing::debug!(
            "should_play_bundle check: override={:?}, device={:?}, result={}",
            self.config.tts.r#override,
            self.current_device,
            should_play
        );
        if !should_play {
            return;
        }

        // Resolve bundle target before borrowing `drill` further. Two
        // separate `&self.study` borrows are issued sequentially so the
        // borrow checker is happy.
        let active_id = self.study.progress().active_course_id.clone();
        let sentence_order = self.study.current_sentence().map(|s| s.order);
        let bundle_target: Option<(String, u32, u32)> = match (active_id, sentence_order) {
            (Some(cid), Some(order)) => Some((cid, order, drill.stage)),
            _ => None,
        };

        if let Some((cid, order, stage)) = bundle_target {
            if let Ok(path) =
                crate::audio::bundle::bundle_path(&self.data_paths.courses_dir, &cid, order, stage)
            {
                if path.exists() {
                    let player = Arc::clone(&self.bundle_player);
                    tokio::spawn(async move {
                        if let Err(e) = player.play(&path).await {
                            tracing::warn!("bundle playback failed: {e}");
                        }
                    });
                    return;
                }
            }
        }

        // TTS-only gates: session and credentials are only relevant for the
        // fall-through path; they must not block bundle playback (spec §3).
        if self.tts_session_disabled {
            tracing::debug!("TTS session disabled, skipping TTS");
            return;
        }
        if !self.tts_has_creds() {
            tracing::debug!("No iFlytek creds, skipping TTS");
            return;
        }

        let text = drill.english.clone();
        self.speak_via_tts(text);
    }

    fn speak_via_tts(&self, text: String) {
        tracing::info!("Spawning TTS task for text: {}", text);
        let speaker = Arc::clone(&self.speaker);
        let last_error = Arc::clone(&self.last_tts_error);
        let task_tx = self.task_tx.clone();
        tokio::spawn(async move {
            let result = speaker.speak(&text).await;
            match result {
                Ok(()) => {
                    *last_error.lock().await = None;
                    let _ = task_tx.send(TaskMsg::TtsSpeakResult(Ok(()))).await;
                }
                Err(e) => {
                    let is_auth = matches!(e, crate::tts::speaker::TtsError::Auth(_));
                    let message = format!("{}", e);
                    *last_error.lock().await = Some(message.clone());
                    let _ = task_tx
                        .send(TaskMsg::TtsSpeakResult(Err(
                            crate::ui::task_msg::TtsSpeakErr { message, is_auth },
                        )))
                        .await;
                }
            }
        });
    }

    pub fn open_course_list(&mut self) {
        use crate::storage::course::list_courses;
        use crate::ui::course_list::CourseListState;
        let metas = list_courses(&self.data_paths.courses_dir).unwrap_or_default();
        self.course_list = Some(CourseListState::new(metas, self.study.progress()));
        self.screen = Screen::CourseList;
    }

    fn switch_to_course(&mut self, new_id: String) {
        use crate::storage::course::load_course;
        // Best-effort save before switching.
        if let Err(e) = self.study.progress().save(&self.data_paths.progress_file) {
            eprintln!("Failed to save progress before switch: {e}");
        }
        let course = match load_course(&self.data_paths.courses_dir, &new_id) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to load course {new_id}: {e}");
                // Stay on list; leave state unchanged.
                return;
            }
        };
        self.study.progress_mut().active_course_id = Some(new_id);
        if let Err(e) = self.study.progress().save(&self.data_paths.progress_file) {
            eprintln!("Failed to save progress after switch: {e}");
        }
        let progress = self.study.progress().clone();
        self.study = crate::ui::study::StudyState::new(Some(course), progress);
        self.course_list = None;
        self.screen = Screen::Study;
        self.speak_current_drill();
    }

    pub fn on_tick(&mut self) {
        self.blink_counter += 1;
        if self.blink_counter >= 33 {
            self.blink_counter = 0;
            self.cursor_visible = !self.cursor_visible;
        }
        // Device probe every ~62 ticks ≈ 1 second (tick cadence = 16ms).
        self.device_probe_counter = self.device_probe_counter.saturating_add(1);
        if self.device_probe_counter >= 62 {
            self.device_probe_counter = 0;
            let task_tx = self.task_tx.clone();
            tokio::task::spawn_blocking(move || {
                let kind = crate::tts::device::detect_output_kind().unwrap_or(OutputKind::Unknown);
                let _ = task_tx.blocking_send(TaskMsg::DeviceDetected(kind));
            });
        }
        // Auto-advance after a correct answer (0.5s linger).
        if matches!(self.screen, Screen::Study) {
            self.tick_advance();
        }
    }

    fn tick_advance(&mut self) {
        let now = self.clock.now();
        if matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes) {
            if !self.study.is_advance_due(now) {
                return;
            }
            self.mistakes.advance_session();
            self.save_mistakes();
            if self.mistakes.peek_current_drill().is_some() {
                self.enter_mistakes_mode_at_current_drill();
            } else {
                // Session finished.
                self.info_banner = Some(MISTAKES_DONE_BANNER.into());
                self.enter_course_mode();
            }
            self.speak_current_drill();
        } else {
            if self.study.auto_advance_if_due(now) {
                let _ = self.study.progress().save(&self.data_paths.progress_file);
                self.speak_current_drill();
            }
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
                Screen::ConfigWizard => self.handle_config_wizard_key(key),
                Screen::CourseList => self.handle_course_list_key(key),
                Screen::TtsStatus => {
                    if key.code == KeyCode::Esc {
                        self.screen = Screen::Study;
                    }
                }
                Screen::Doctor => {
                    if key.code == KeyCode::Esc {
                        self.doctor_results = None;
                        self.screen = Screen::Study;
                    }
                }
            },
            Event::Paste(text) => {
                if let Screen::Generate = self.screen {
                    if let Some(GenerateSubstate::Pasting(ref mut p)) = self.generate {
                        p.textarea.insert_str(&text);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn on_task_msg(&mut self, msg: TaskMsg) {
        match msg {
            TaskMsg::Generate(progress) => self.handle_generate_progress(progress),
            TaskMsg::Wizard(m) => self.handle_wizard_task_msg(m),
            TaskMsg::DeviceDetected(kind) => {
                self.current_device = kind;
            }
            TaskMsg::TtsSpeakResult(result) => match result {
                Ok(()) => {
                    self.tts_failure_count = 0;
                    // Re-enable TTS session if it was previously disabled
                    if self.tts_session_disabled {
                        self.tts_session_disabled = false;
                        tracing::info!("TTS session re-enabled after successful synthesis");
                    }
                }
                Err(e) => {
                    self.tts_failure_count += 1;
                    tracing::warn!(
                        failure_count = self.tts_failure_count,
                        is_auth = e.is_auth,
                        error = %e.message,
                        "TTS synthesis failed"
                    );
                    // Auth/license failures don't self-heal — disable the
                    // session immediately so we don't keep hammering the API
                    // and the user gets a clear signal something is wrong.
                    if e.is_auth {
                        self.tts_session_disabled = true;
                        tracing::warn!("TTS session disabled (auth failure: {})", e.message);
                    } else if self.tts_failure_count >= 5 {
                        self.tts_session_disabled = true;
                        tracing::warn!("TTS session disabled after 5 consecutive failures");
                    }
                }
            },
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
                self.speak_current_drill();
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
        // Clear info banner on any key
        if self.info_banner.is_some() {
            self.info_banner = None;
            return;
        }

        // Command+Backspace: delete to beginning of line
        if key.modifiers.contains(KeyModifiers::SUPER) && key.code == KeyCode::Backspace {
            self.study.delete_to_line_start();
            return;
        }

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
        if key.code == KeyCode::Esc
            && matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes)
        {
            // Park session as-is and drop back to course mode. Next launch /
            // /mistakes resumes from session.next_index.
            self.save_mistakes();
            self.info_banner = Some("Review paused (resume with /mistakes)".into());
            self.enter_course_mode();
            self.speak_current_drill();
            return;
        }
        match self.study.feedback() {
            FeedbackState::Correct => {
                let _ = self.study.progress().save(&self.data_paths.progress_file);
                self.study.advance();
                self.speak_current_drill();
            }
            FeedbackState::Wrong => {
                // Any key press clears input and restarts
                self.study.clear_and_restart();
                self.speak_current_drill();
            }
            FeedbackState::Typing => match key.code {
                KeyCode::Char(c) => self.study.type_char(c),
                KeyCode::Backspace => self.study.backspace(),
                KeyCode::Enter => {
                    if self.study.input().is_empty() {
                        self.speak_current_drill();
                    } else {
                        let outcome = self.study.submit(self.clock.as_ref());
                        if let Some(o) = outcome {
                            self.handle_submit_outcome(o);
                        }
                    }
                }
                KeyCode::Tab => {
                    if matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes) {
                        // Skip in mistakes mode: advance via session queue, no verdict recorded.
                        self.mistakes.advance_session();
                        self.save_mistakes();
                        if self.mistakes.peek_current_drill().is_some() {
                            self.enter_mistakes_mode_at_current_drill();
                        } else {
                            self.info_banner = Some(MISTAKES_DONE_BANNER.into());
                            self.enter_course_mode();
                        }
                    } else {
                        self.study.skip();
                    }
                    self.speak_current_drill();
                }
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
                    if let Some((cmd, args)) = p.confirm() {
                        self.execute_command(cmd, &args);
                    }
                }
                if !self.should_quit {
                    self.palette = None;
                    if matches!(self.screen, Screen::Palette) {
                        self.screen = Screen::Study;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, _key: KeyEvent) {
        self.screen = Screen::Study;
    }

    fn handle_generate_key(&mut self, key: KeyEvent) {
        tracing::debug!("handle_generate_key: {:?}", key);
        let Some(ref gen_state) = self.generate else {
            return;
        };

        match gen_state {
            GenerateSubstate::Pasting(_) => {
                // Ctrl+C: quit
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    self.quit();
                    return;
                }
                // Ctrl+D or F5: submit
                let is_submit = (key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('d'))
                    || key.code == KeyCode::F(5);
                if is_submit {
                    let can_submit = if let Some(GenerateSubstate::Pasting(p)) = &self.generate {
                        p.can_submit(self.config.generation.max_article_bytes)
                    } else {
                        false
                    };
                    if can_submit {
                        let text = if let Some(GenerateSubstate::Pasting(p)) = &self.generate {
                            p.get_text()
                        } else {
                            return;
                        };
                        self.submit_generate(text);
                    }
                    return;
                }
                if key.code == KeyCode::Esc {
                    self.generate = None;
                    self.screen = Screen::Study;
                    return;
                }
                // Delegate all other keys to tui-textarea
                if let Some(GenerateSubstate::Pasting(ref mut p)) = self.generate {
                    p.textarea.input(key);
                }
            }
            GenerateSubstate::Running(_) => {
                if key.code == KeyCode::Esc {
                    if let Some(GenerateSubstate::Running(ref r)) = self.generate {
                        r.cancel_token.cancel();
                    }
                    self.generate = Some(GenerateSubstate::Pasting(Box::default()));
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
                            pasting.textarea.insert_str(&article_text);
                            self.generate = Some(GenerateSubstate::Pasting(Box::new(pasting)));
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
                        self.mistakes.purge_course(&course_id);
                        self.save_mistakes();
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

            let level_desc = config.generation.english_level.prompt_description();

            match reflexion
                .generate(&article, level_desc, &existing_ids, Some(progress_tx_clone))
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
                            saved_to,
                            ..
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

    fn execute_command(&mut self, cmd: &Command, args: &[String]) {
        match cmd.name {
            "quit" | "q" => self.quit(),
            "skip" => {
                self.study.skip();
                self.speak_current_drill();
            }
            "help" => self.screen = Screen::Help,
            "import" => {
                self.generate = Some(GenerateSubstate::Pasting(Box::default()));
                self.screen = Screen::Generate;
            }
            "config" => {
                self.open_wizard(crate::ui::config_wizard::WizardOrigin::Command);
            }
            "list" => self.open_course_list(),
            "tts" => self.execute_tts(args),
            "delete" => {
                if let Some(course) = self.study.current_course() {
                    self.delete_confirming = Some(course.title.clone());
                    self.screen = Screen::DeleteConfirm;
                }
            }
            "logs" => self.execute_logs(),
            "mistakes" => {
                let today = self.clock.today_local();
                self.mistakes.ensure_session_force(today);
                self.save_mistakes();
                if self.mistakes.peek_current_drill().is_some() {
                    self.enter_mistakes_mode_at_current_drill();
                    self.speak_current_drill();
                } else {
                    self.info_banner = Some("🎉 No reviews today".into());
                }
            }
            "doctor" => self.execute_doctor(),
            _ => {}
        }
    }

    fn execute_tts(&mut self, args: &[String]) {
        use crate::config::TtsOverride;
        let first = args.first().map(|s| s.as_str()).unwrap_or("");
        match first {
            "on" => self.set_tts_override(TtsOverride::On),
            "off" => self.set_tts_override(TtsOverride::Off),
            "auto" => self.set_tts_override(TtsOverride::Auto),
            "clear-cache" => {
                let _ = crate::tts::clear_cache(&self.data_paths.tts_cache_dir);
            }
            "" => {
                self.screen = Screen::TtsStatus;
            }
            _ => {}
        }
    }

    fn set_tts_override(&mut self, new_mode: crate::config::TtsOverride) {
        // Session-only: do not persist to config.toml. The TOML-level default
        // is a deliberate user choice; palette toggles are transient overrides
        // for the running process.
        self.config.tts.r#override = new_mode;
    }

    fn execute_logs(&mut self) {
        let log_path = self.data_paths.root.join("inkworm.log");
        let path_str = log_path.display().to_string();

        let _ = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(path_str.as_bytes())?;
                }
                child.wait()
            });

        self.info_banner = Some(format!("Copied to clipboard: {}", path_str));
        self.screen = Screen::Study;
    }

    fn execute_doctor(&mut self) {
        let results = crate::ui::doctor::run_checks(
            &self.config,
            &self.data_paths,
            Some(self.speaker.as_ref()),
            self.current_device,
        );
        self.doctor_results = Some(results);
        self.screen = Screen::Doctor;
    }

    fn quit(&mut self) {
        self.speaker.cancel();
        self.bundle_player.cancel();
        let _ = self.study.progress().save(&self.data_paths.progress_file);
        self.should_quit = true;
    }

    /// Draw the shell prompt header on row 0 and the status bar on the
    /// last row, returning the inner Rect available for the study UI.
    fn render_chrome(&self, frame: &mut Frame) -> ratatui::layout::Rect {
        use ratatui::layout::Rect;
        let area = frame.area();
        if area.height < 3 {
            // Too small to spare two rows for chrome; skip it.
            return area;
        }
        let header_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        let status_area = Rect {
            x: area.x,
            y: area.y + area.height - 1,
            width: area.width,
            height: 1,
        };
        let inner = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height - 2,
        };

        let header_line = self.shell_header.render(area.width);
        frame.render_widget(ratatui::widgets::Paragraph::new(header_line), header_area);

        let course_id = self.study.current_course().map(|c| c.id.as_str());
        let summary = self
            .study
            .current_course()
            .map(|c| crate::ui::shell_chrome::ProgressSummary::compute(c, self.study.progress()));
        let badge = if matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes) {
            self.mistakes.session_progress().and_then(|p| {
                let drill_ref = self.mistakes.current_drill_ref()?;
                let streak = self
                    .mistakes
                    .entries
                    .iter()
                    .find(|e| e.drill == drill_ref)
                    .map(|e| e.streak_days)
                    .unwrap_or(0);
                Some(crate::ui::shell_chrome::MistakesBadge {
                    round: p.round,
                    total_rounds: 2,
                    index: p.index,
                    total: p.total,
                    streak_days: streak,
                    streak_target: 3,
                })
            })
        } else {
            None
        };
        let status_line = crate::ui::shell_chrome::build_status_line_with_mistakes(
            area.width, course_id, summary, badge,
        );
        frame.render_widget(ratatui::widgets::Paragraph::new(status_line), status_area);

        inner
    }

    /// Stack the bottom-of-screen banners: a red TTS-disabled banner (when
    /// the session is paused) above the yellow info banner. Each takes one
    /// row; older info_banner sits on the very last row to preserve current
    /// layout, the TTS banner — which we want maximally visible — sits one
    /// row above it (or on the last row if there's no info_banner).
    fn render_bottom_banners(&self, frame: &mut Frame, inner: ratatui::layout::Rect) {
        use ratatui::{
            layout::Rect,
            style::{Color, Style},
            text::Line,
            widgets::Paragraph,
        };
        let mut row_from_bottom = 0u16;
        let last_row = inner.y + inner.height.saturating_sub(1);

        if let Some(ref banner) = self.info_banner {
            let para = Paragraph::new(Line::from(banner.as_str()))
                .style(Style::default().fg(Color::Yellow))
                .centered();
            frame.render_widget(para, Rect::new(inner.x, last_row, inner.width, 1));
            row_from_bottom += 1;
        }

        if self.tts_session_disabled {
            let reason = self
                .last_tts_error
                .try_lock()
                .ok()
                .and_then(|g| g.clone())
                .unwrap_or_else(|| "session paused".into());
            let text = format!("🔇 TTS disabled — {}. See /tts for details.", reason);
            let y = last_row.saturating_sub(row_from_bottom);
            let para = Paragraph::new(Line::from(text))
                .style(Style::default().fg(Color::Red))
                .centered();
            frame.render_widget(para, Rect::new(inner.x, y, inner.width, 1));
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        match &self.screen {
            Screen::Study => {
                let inner = self.render_chrome(frame);
                crate::ui::study::render_study(frame, inner, &self.study, self.cursor_visible);
                self.render_bottom_banners(frame, inner);
            }
            Screen::Palette => {
                let inner = self.render_chrome(frame);
                crate::ui::study::render_study(frame, inner, &self.study, self.cursor_visible);
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
                let inner = self.render_chrome(frame);
                crate::ui::study::render_study(frame, inner, &self.study, self.cursor_visible);
                if let Some(ref title) = self.delete_confirming {
                    render_delete_confirm(frame, title);
                }
            }
            Screen::ConfigWizard => {
                if let Some(ref state) = self.config_wizard {
                    crate::ui::config_wizard::render_config_wizard(
                        frame,
                        state,
                        self.cursor_visible,
                    );
                }
            }
            Screen::CourseList => {
                let inner = self.render_chrome(frame);
                crate::ui::study::render_study(frame, inner, &self.study, self.cursor_visible);
                if let Some(ref state) = self.course_list {
                    crate::ui::course_list::render_course_list(frame, state);
                }
            }
            Screen::TtsStatus => {
                let inner = self.render_chrome(frame);
                crate::ui::study::render_study(frame, inner, &self.study, self.cursor_visible);
                let cache_stats = crate::tts::cache::cache_stats(&self.data_paths.tts_cache_dir);
                let last_error = self
                    .last_tts_error
                    .try_lock()
                    .ok()
                    .and_then(|guard| guard.clone());
                crate::ui::tts_status::render_tts_status(
                    frame,
                    &self.config.tts,
                    self.current_device,
                    last_error,
                    cache_stats,
                    self.tts_session_disabled,
                );
            }
            Screen::Doctor => {
                let inner = self.render_chrome(frame);
                crate::ui::study::render_study(frame, inner, &self.study, self.cursor_visible);
                if let Some(ref results) = self.doctor_results {
                    crate::ui::doctor::render_doctor(frame, results);
                }
            }
        }
    }

    fn handle_config_wizard_key(&mut self, key: KeyEvent) {
        use crate::ui::config_wizard::{BackOutcome, CommitOutcome};

        let is_testing = self
            .config_wizard
            .as_ref()
            .and_then(|s| s.testing.as_ref())
            .is_some();

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit();
            return;
        }

        if is_testing {
            if key.code == KeyCode::Esc {
                if let Some(ref mut state) = self.config_wizard {
                    if let Some(ref t) = state.testing {
                        t.cancel_token.cancel();
                    }
                    state.testing = None;
                }
            }
            return;
        }

        match key.code {
            KeyCode::Esc => {
                let outcome = self
                    .config_wizard
                    .as_mut()
                    .map(|s| s.back())
                    .unwrap_or(BackOutcome::NoOp);
                match outcome {
                    BackOutcome::Back | BackOutcome::NoOp => {}
                    BackOutcome::Abort => {
                        self.config_wizard = None;
                        self.screen = Screen::Study;
                    }
                }
            }
            KeyCode::Enter => {
                let outcome = self
                    .config_wizard
                    .as_mut()
                    .map(|s| s.commit())
                    .unwrap_or(CommitOutcome::Invalid);
                match outcome {
                    CommitOutcome::ProbeConnectivity => {
                        self.spawn_connectivity_test();
                    }
                    CommitOutcome::Advance | CommitOutcome::Invalid => {}
                    CommitOutcome::ProbeTts => {
                        self.spawn_tts_probe();
                    }
                    CommitOutcome::SaveConfig => {
                        self.save_wizard_config();
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut state) = self.config_wizard {
                    state.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut state) = self.config_wizard {
                    state.type_char(c);
                }
            }
            _ => {}
        }
    }

    fn handle_course_list_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.course_list = None;
                self.screen = Screen::Study;
            }
            KeyCode::Up => {
                if let Some(s) = &mut self.course_list {
                    s.select_prev();
                }
            }
            KeyCode::Down => {
                if let Some(s) = &mut self.course_list {
                    s.select_next();
                }
            }
            KeyCode::PageUp => {
                if let Some(s) = &mut self.course_list {
                    s.page_up(5);
                }
            }
            KeyCode::PageDown => {
                if let Some(s) = &mut self.course_list {
                    s.page_down(5);
                }
            }
            KeyCode::Enter => {
                let chosen_id = self
                    .course_list
                    .as_ref()
                    .and_then(|s| s.selected_item())
                    .map(|i| i.meta.id.clone());
                if let Some(id) = chosen_id {
                    self.switch_to_course(id);
                }
            }
            _ => {}
        }
    }

    fn spawn_connectivity_test(&mut self) {
        use crate::ui::config_wizard::{probe_llm, TestingState};
        use crate::ui::task_msg::WizardTaskMsg;

        let llm = match self.config_wizard.as_ref() {
            Some(s) => s.draft.llm.clone(),
            None => return,
        };
        let cancel = CancellationToken::new();
        if let Some(state) = self.config_wizard.as_mut() {
            state.testing = Some(TestingState {
                cancel_token: cancel.clone(),
            });
        }
        let task_tx = self.task_tx.clone();
        tokio::spawn(async move {
            let msg = match probe_llm(llm, cancel).await {
                Ok(()) => WizardTaskMsg::ConnectivityOk,
                Err(e) => WizardTaskMsg::ConnectivityFailed(e),
            };
            let _ = task_tx.send(TaskMsg::Wizard(msg)).await;
        });
    }

    fn spawn_tts_probe(&mut self) {
        use crate::ui::config_wizard::{probe_tts, TestingState};
        use crate::ui::task_msg::WizardTaskMsg;

        let iflytek = match self.config_wizard.as_ref() {
            Some(s) => s.draft.tts.iflytek.clone(),
            None => return,
        };
        let cancel = CancellationToken::new();
        if let Some(state) = self.config_wizard.as_mut() {
            state.testing = Some(TestingState {
                cancel_token: cancel.clone(),
            });
        }
        let task_tx = self.task_tx.clone();
        tokio::spawn(async move {
            let msg = match probe_tts(iflytek, cancel).await {
                Ok(()) => WizardTaskMsg::TtsProbeOk,
                Err(e) => WizardTaskMsg::TtsProbeFailed(e),
            };
            let _ = task_tx.send(TaskMsg::Wizard(msg)).await;
        });
    }

    fn save_wizard_config(&mut self) {
        let Some(wizard) = self.config_wizard.as_mut() else {
            return;
        };
        wizard.testing = None;

        let mut merged = Config::load(&self.data_paths.config_file).unwrap_or_default();
        merged.llm = wizard.draft.llm.clone();
        merged.tts = wizard.draft.tts.clone();
        match merged.write_atomic(&self.data_paths.config_file) {
            Ok(()) => {
                self.config = merged;
                self.config_wizard = None;
                self.screen = Screen::Study;
            }
            Err(e) => {
                let app_err = crate::error::AppError::Config(e);
                wizard.error = Some(user_message(&app_err));
            }
        }
    }

    fn handle_wizard_task_msg(&mut self, msg: crate::ui::task_msg::WizardTaskMsg) {
        use crate::ui::error_banner::user_message;
        use crate::ui::task_msg::WizardTaskMsg;

        let Some(wizard) = self.config_wizard.as_mut() else {
            return;
        };
        wizard.testing = None;

        match msg {
            WizardTaskMsg::ConnectivityOk => {
                // Advance to TtsEnable step (wizard will handle the flow from there).
                wizard.step = crate::ui::config_wizard::WizardStep::TtsEnable;
                wizard.input = if wizard.tts_enabled { "y" } else { "n" }.to_string();
            }
            WizardTaskMsg::ConnectivityFailed(e) => {
                wizard.error = Some(user_message(&e));
            }
            WizardTaskMsg::TtsProbeOk => {
                self.save_wizard_config();
            }
            WizardTaskMsg::TtsProbeFailed(e) => {
                if let Some(w) = self.config_wizard.as_mut() {
                    w.error = Some(user_message(&e));
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
