//! Config wizard: multi-step first-run / `/config` flow for LLM and TTS setup.
//! Connectivity probe and rendering live in this file (probe in §Probe block, render in §Render block).

use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tokio_util::sync::CancellationToken;

use crate::config::{Config, LlmConfig};
use crate::error::AppError;
use crate::llm::client::{LlmClient, ReqwestClient};
use crate::llm::types::{ChatMessage, ChatRequest, Role};
use crate::ui::error_banner::UserMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Endpoint,
    ApiKey,
    Model,
    TtsEnable,
    TtsAppId,
    TtsApiKey,
    TtsApiSecret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardOrigin {
    FirstRun,
    Command,
}

#[derive(Debug)]
pub struct TestingState {
    pub cancel_token: CancellationToken,
}

#[derive(Debug)]
pub struct WizardState {
    pub step: WizardStep,
    pub origin: WizardOrigin,
    pub draft: Config,
    pub input: String,
    pub testing: Option<TestingState>,
    pub error: Option<UserMessage>,
    pub tts_enabled: bool,
}

/// Outcome of `WizardState::commit` — tells App what to do next.
#[derive(Debug)]
pub enum CommitOutcome {
    /// Advance to next step (input already seeded with draft value for the new step).
    Advance,
    /// On Model step — spawn connectivity test.
    ProbeConnectivity,
    /// On TtsApiSecret step — spawn TTS connectivity test.
    ProbeTts,
    /// Save config without TTS probe (user declined TTS).
    SaveConfig,
    /// Input was invalid (e.g., empty). `error` is now set; stay on same step.
    Invalid,
}

/// Outcome of `WizardState::back` — tells App what to do next.
#[derive(Debug)]
pub enum BackOutcome {
    /// Moved to previous step (input seeded).
    Back,
    /// Stayed (FirstRun on Endpoint).
    NoOp,
    /// Abort wizard (Command on Endpoint).
    Abort,
}

impl WizardState {
    pub fn new(origin: WizardOrigin, draft: Config) -> Self {
        let input = draft.llm.base_url.clone();
        let tts_enabled = draft.tts.enabled;
        Self {
            step: WizardStep::Endpoint,
            origin,
            draft,
            input,
            testing: None,
            error: None,
            tts_enabled,
        }
    }

    pub fn total_steps(&self) -> u8 {
        if self.tts_enabled {
            7
        } else {
            4
        }
    }

    pub fn step_number(&self) -> u8 {
        match self.step {
            WizardStep::Endpoint => 1,
            WizardStep::ApiKey => 2,
            WizardStep::Model => 3,
            WizardStep::TtsEnable => 4,
            WizardStep::TtsAppId => 5,
            WizardStep::TtsApiKey => 6,
            WizardStep::TtsApiSecret => 7,
        }
    }

    pub fn type_char(&mut self, c: char) {
        self.error = None;
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.error = None;
        self.input.pop();
    }

    /// Commit current step. On Endpoint/ApiKey: Advance; on Model: ProbeConnectivity.
    pub fn commit(&mut self) -> CommitOutcome {
        self.error = None;
        let trimmed = self.input.trim();
        if trimmed.is_empty() && self.step != WizardStep::TtsEnable {
            let label = match self.step {
                WizardStep::Endpoint => "Endpoint cannot be empty",
                WizardStep::ApiKey => "API key cannot be empty",
                WizardStep::Model => "Model cannot be empty",
                WizardStep::TtsAppId => "App ID cannot be empty",
                WizardStep::TtsApiKey => "API key cannot be empty",
                WizardStep::TtsApiSecret => "API secret cannot be empty",
                WizardStep::TtsEnable => unreachable!(),
            };
            self.error = Some(UserMessage {
                headline: label.to_string(),
                hint: String::new(),
                severity: crate::ui::error_banner::Severity::Error,
            });
            return CommitOutcome::Invalid;
        }
        match self.step {
            WizardStep::Endpoint => {
                self.draft.llm.base_url = trimmed.to_string();
                self.step = WizardStep::ApiKey;
                self.input = self.draft.llm.api_key.clone();
                CommitOutcome::Advance
            }
            WizardStep::ApiKey => {
                self.draft.llm.api_key = trimmed.to_string();
                self.step = WizardStep::Model;
                self.input = self.draft.llm.model.clone();
                CommitOutcome::Advance
            }
            WizardStep::Model => {
                self.draft.llm.model = trimmed.to_string();
                CommitOutcome::ProbeConnectivity
            }
            WizardStep::TtsEnable => {
                let lower = trimmed.to_lowercase();
                if lower == "y" {
                    self.tts_enabled = true;
                    self.draft.tts.enabled = true;
                    self.step = WizardStep::TtsAppId;
                    self.input = self.draft.tts.iflytek.app_id.clone();
                    CommitOutcome::Advance
                } else if lower == "n" {
                    self.tts_enabled = false;
                    self.draft.tts.enabled = false;
                    CommitOutcome::SaveConfig
                } else {
                    self.error = Some(UserMessage {
                        headline: "Type y or n".to_string(),
                        hint: String::new(),
                        severity: crate::ui::error_banner::Severity::Error,
                    });
                    CommitOutcome::Invalid
                }
            }
            WizardStep::TtsAppId => {
                self.draft.tts.iflytek.app_id = trimmed.to_string();
                self.step = WizardStep::TtsApiKey;
                self.input = self.draft.tts.iflytek.api_key.clone();
                CommitOutcome::Advance
            }
            WizardStep::TtsApiKey => {
                self.draft.tts.iflytek.api_key = trimmed.to_string();
                self.step = WizardStep::TtsApiSecret;
                self.input = self.draft.tts.iflytek.api_secret.clone();
                CommitOutcome::Advance
            }
            WizardStep::TtsApiSecret => {
                self.draft.tts.iflytek.api_secret = trimmed.to_string();
                CommitOutcome::ProbeTts
            }
        }
    }

    /// Go back one step. See BackOutcome for semantics.
    pub fn back(&mut self) -> BackOutcome {
        self.error = None;
        match self.step {
            WizardStep::Endpoint => match self.origin {
                WizardOrigin::FirstRun => BackOutcome::NoOp,
                WizardOrigin::Command => BackOutcome::Abort,
            },
            WizardStep::ApiKey => {
                self.step = WizardStep::Endpoint;
                self.input = self.draft.llm.base_url.clone();
                BackOutcome::Back
            }
            WizardStep::Model => {
                self.step = WizardStep::ApiKey;
                self.input = self.draft.llm.api_key.clone();
                BackOutcome::Back
            }
            WizardStep::TtsEnable => {
                self.step = WizardStep::Model;
                self.input = self.draft.llm.model.clone();
                BackOutcome::Back
            }
            WizardStep::TtsAppId => {
                self.step = WizardStep::TtsEnable;
                self.input = if self.tts_enabled { "y" } else { "n" }.to_string();
                BackOutcome::Back
            }
            WizardStep::TtsApiKey => {
                self.step = WizardStep::TtsAppId;
                self.input = self.draft.tts.iflytek.app_id.clone();
                BackOutcome::Back
            }
            WizardStep::TtsApiSecret => {
                self.step = WizardStep::TtsApiKey;
                self.input = self.draft.tts.iflytek.api_key.clone();
                BackOutcome::Back
            }
        }
    }
}

/// Fire a minimal 1-token chat request to verify credentials and model work.
/// Maps any LlmError into AppError. Cancellation via the token returns
/// AppError::Cancelled.
pub async fn probe_llm(llm: LlmConfig, cancel: CancellationToken) -> Result<(), AppError> {
    let client = ReqwestClient::new(
        llm.base_url.clone(),
        llm.api_key.clone(),
        Duration::from_secs(llm.request_timeout_secs),
    )?;

    let req = ChatRequest {
        model: llm.model.clone(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: "ping".into(),
        }],
        temperature: Some(0.0),
        max_tokens: Some(1),
        response_format: None,
    };

    match client.chat(req, cancel).await {
        Ok(_content) => Ok(()),
        Err(crate::llm::error::LlmError::Cancelled) => Err(AppError::Cancelled),
        Err(e) => Err(AppError::Llm(e)),
    }
}

/// Title line for the wizard frame.
pub fn wizard_title_dynamic(state: &WizardState) -> String {
    let n = state.step_number();
    let total = state.total_steps();
    format!("inkworm — setup ({n} / {total})")
}

/// Step-specific label.
pub fn wizard_step_label(step: WizardStep) -> &'static str {
    match step {
        WizardStep::Endpoint => "LLM endpoint",
        WizardStep::ApiKey => "LLM API key",
        WizardStep::Model => "LLM model",
        WizardStep::TtsEnable => "Enable TTS? (y/n)",
        WizardStep::TtsAppId => "iFlytek App ID",
        WizardStep::TtsApiKey => "iFlytek API Key",
        WizardStep::TtsApiSecret => "iFlytek API Secret",
    }
}

/// Display-ready input — masks the ApiKey, TtsApiKey, and TtsApiSecret steps.
pub fn mask_for_display(input: &str, step: WizardStep) -> String {
    match step {
        WizardStep::ApiKey | WizardStep::TtsApiKey | WizardStep::TtsApiSecret => {
            "*".repeat(input.chars().count())
        }
        _ => input.to_string(),
    }
}

/// Hint line at the bottom of the wizard.
pub fn wizard_hint(step: WizardStep, origin: WizardOrigin, testing: bool) -> &'static str {
    if testing {
        return "Testing connectivity…     Esc · cancel";
    }
    match (step, origin) {
        (WizardStep::Endpoint, WizardOrigin::FirstRun) => "Enter · next     Ctrl+C · quit",
        (WizardStep::Endpoint, WizardOrigin::Command) => "Enter · next     Esc · cancel",
        (WizardStep::ApiKey, _) => "Enter · next     Esc · back",
        (WizardStep::Model, _) => "Enter · test and save     Esc · back",
        (WizardStep::TtsEnable, _) => "Enter · next     Esc · back",
        (WizardStep::TtsAppId, _) => "Enter · next     Esc · back",
        (WizardStep::TtsApiKey, _) => "Enter · next     Esc · back",
        (WizardStep::TtsApiSecret, _) => "Enter · test and save     Esc · back",
    }
}

pub fn render_config_wizard(frame: &mut Frame, state: &WizardState, cursor_visible: bool) {
    let area = frame.area();

    let title = wizard_title_dynamic(state);
    let label = wizard_step_label(state.step);
    let rendered_input = mask_for_display(&state.input, state.step);
    let cursor_glyph = if cursor_visible { "_" } else { " " };
    let hint = wizard_hint(state.step, state.origin, state.testing.is_some());

    let has_error = state.error.is_some();
    let block_height: u16 = if has_error { 8 } else { 6 };
    let top = area.height.saturating_sub(block_height) / 2;
    let left = area.width / 5;
    let width = area.width.saturating_sub(left * 2);

    let title_line = Paragraph::new(Span::styled(
        title,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(title_line, Rect::new(left, top, width, 1));

    let label_line = Paragraph::new(Span::styled(
        label,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(label_line, Rect::new(left, top + 2, width, 1));

    let input_line = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::DarkGray)),
        Span::styled(rendered_input, Style::default().fg(Color::White)),
        Span::styled(cursor_glyph, Style::default().fg(Color::White)),
    ]));
    frame.render_widget(input_line, Rect::new(left, top + 3, width, 1));

    let hint_line = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    frame.render_widget(hint_line, Rect::new(left, top + 5, width, 1));

    if let Some(ref err) = state.error {
        let color = match err.severity {
            crate::ui::error_banner::Severity::Error => Color::Red,
            crate::ui::error_banner::Severity::Warning => Color::Yellow,
            crate::ui::error_banner::Severity::Info => Color::Blue,
        };
        let err_line = Paragraph::new(Span::styled(
            err.headline.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(err_line, Rect::new(left, top + 7, width, 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn new_wiz() -> WizardState {
        WizardState::new(WizardOrigin::FirstRun, Config::default())
    }

    #[test]
    fn new_starts_on_endpoint_with_default_url() {
        let w = new_wiz();
        assert_eq!(w.step, WizardStep::Endpoint);
        assert_eq!(w.input, "https://api.openai.com/v1");
    }

    #[test]
    fn commit_endpoint_advances_to_apikey() {
        let mut w = new_wiz();
        w.input = "https://example.com/v1".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Advance));
        assert_eq!(w.step, WizardStep::ApiKey);
        assert_eq!(w.draft.llm.base_url, "https://example.com/v1");
    }

    #[test]
    fn commit_empty_sets_error_and_stays() {
        let mut w = new_wiz();
        w.input = "   ".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Invalid));
        assert_eq!(w.step, WizardStep::Endpoint);
        assert!(w.error.is_some());
    }

    #[test]
    fn commit_model_requests_probe() {
        let mut w = new_wiz();
        w.input = "https://example.com/v1".into();
        w.commit();
        w.input = "sk-123".into();
        w.commit();
        assert_eq!(w.step, WizardStep::Model);
        w.input = "gpt-4o-mini".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::ProbeConnectivity));
        assert_eq!(w.draft.llm.model, "gpt-4o-mini");
    }

    #[test]
    fn back_from_apikey_returns_to_endpoint_with_seeded_input() {
        let mut w = new_wiz();
        w.input = "https://x/v1".into();
        w.commit();
        w.input = "sk-typed".into();
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::Back));
        assert_eq!(w.step, WizardStep::Endpoint);
        assert_eq!(w.input, "https://x/v1");
    }

    #[test]
    fn back_on_endpoint_firstrun_is_noop() {
        let mut w = WizardState::new(WizardOrigin::FirstRun, Config::default());
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::NoOp));
        assert_eq!(w.step, WizardStep::Endpoint);
    }

    #[test]
    fn back_on_endpoint_command_aborts() {
        let mut w = WizardState::new(WizardOrigin::Command, Config::default());
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::Abort));
    }

    #[test]
    fn type_and_backspace_clear_error() {
        let mut w = new_wiz();
        // Round 1: empty commit → error; type_char clears error.
        w.input.clear();
        w.commit();
        assert!(w.error.is_some());
        w.type_char('a');
        assert!(w.error.is_none());
        // Round 2: empty commit again → error; backspace clears error.
        w.input.clear();
        w.commit();
        assert!(w.error.is_some());
        w.backspace();
        assert!(w.error.is_none());
    }

    #[tokio::test]
    async fn probe_llm_ok_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role":"assistant","content":"pong"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let llm = LlmConfig {
            base_url: server.uri(),
            api_key: "sk-test".into(),
            model: "gpt-4o-mini".into(),
            request_timeout_secs: 5,
            reflexion_budget_secs: 60,
        };
        let res = probe_llm(llm, CancellationToken::new()).await;
        assert!(res.is_ok(), "{res:?}");
    }

    #[tokio::test]
    async fn probe_llm_maps_401_to_llm_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("no"))
            .expect(1)
            .mount(&server)
            .await;

        let llm = LlmConfig {
            base_url: server.uri(),
            api_key: "sk-test".into(),
            model: "gpt-4o-mini".into(),
            request_timeout_secs: 5,
            reflexion_budget_secs: 60,
        };
        let err = probe_llm(llm, CancellationToken::new()).await.unwrap_err();
        assert!(
            matches!(
                err,
                AppError::Llm(crate::llm::error::LlmError::Unauthorized)
            ),
            "{err:?}"
        );
    }

    #[test]
    fn mask_hides_apikey_but_preserves_others() {
        assert_eq!(
            mask_for_display("supersecret", WizardStep::ApiKey),
            "***********"
        );
        assert_eq!(
            mask_for_display("https://x/v1", WizardStep::Endpoint),
            "https://x/v1"
        );
        assert_eq!(
            mask_for_display("gpt-4o-mini", WizardStep::Model),
            "gpt-4o-mini"
        );
    }

    #[test]
    fn mask_counts_unicode_chars_not_bytes() {
        // Each CJK char is multi-byte — we count chars, so mask length = char count.
        assert_eq!(mask_for_display("你好", WizardStep::ApiKey), "**");
    }

    #[test]
    fn tts_enable_y_advances_to_tts_app_id() {
        let mut w = new_wiz();
        w.input = "https://x/v1".into();
        w.commit();
        w.input = "sk-123".into();
        w.commit();
        w.step = WizardStep::TtsEnable;
        w.input = "y".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Advance));
        assert_eq!(w.step, WizardStep::TtsAppId);
        assert!(w.tts_enabled);
    }

    #[test]
    fn tts_enable_n_returns_save_config() {
        let mut w = new_wiz();
        w.step = WizardStep::TtsEnable;
        w.input = "n".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::SaveConfig));
        assert!(!w.tts_enabled);
        assert!(!w.draft.tts.enabled);
    }

    #[test]
    fn tts_enable_invalid_input_stays() {
        let mut w = new_wiz();
        w.step = WizardStep::TtsEnable;
        w.input = "maybe".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Invalid));
        assert!(w.error.is_some());
    }

    #[test]
    fn tts_credential_steps_advance() {
        let mut w = new_wiz();
        w.tts_enabled = true;
        w.step = WizardStep::TtsAppId;
        w.input = "app123".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Advance));
        assert_eq!(w.step, WizardStep::TtsApiKey);
        assert_eq!(w.draft.tts.iflytek.app_id, "app123");

        w.input = "key456".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::Advance));
        assert_eq!(w.step, WizardStep::TtsApiSecret);
        assert_eq!(w.draft.tts.iflytek.api_key, "key456");

        w.input = "sec789".into();
        let outcome = w.commit();
        assert!(matches!(outcome, CommitOutcome::ProbeTts));
        assert_eq!(w.draft.tts.iflytek.api_secret, "sec789");
    }

    #[test]
    fn tts_back_navigation() {
        let mut w = new_wiz();
        w.tts_enabled = true;
        w.step = WizardStep::TtsApiSecret;
        w.draft.tts.iflytek.api_key = "k".into();
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::Back));
        assert_eq!(w.step, WizardStep::TtsApiKey);
        assert_eq!(w.input, "k");

        w.draft.tts.iflytek.app_id = "a".into();
        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::Back));
        assert_eq!(w.step, WizardStep::TtsAppId);
        assert_eq!(w.input, "a");

        let outcome = w.back();
        assert!(matches!(outcome, BackOutcome::Back));
        assert_eq!(w.step, WizardStep::TtsEnable);
    }

    #[test]
    fn total_steps_dynamic() {
        let mut w = new_wiz();
        w.step = WizardStep::TtsEnable;
        w.tts_enabled = false;
        assert_eq!(w.total_steps(), 4);
        w.tts_enabled = true;
        assert_eq!(w.total_steps(), 7);
    }

    #[test]
    fn step_number_sequential() {
        let mut w = new_wiz();
        w.step = WizardStep::Endpoint;
        assert_eq!(w.step_number(), 1);
        w.step = WizardStep::TtsEnable;
        assert_eq!(w.step_number(), 4);
        w.step = WizardStep::TtsApiSecret;
        assert_eq!(w.step_number(), 7);
    }

    #[test]
    fn wizard_title_dynamic_test() {
        let mut w = new_wiz();
        w.step = WizardStep::TtsEnable;
        w.tts_enabled = false;
        assert_eq!(wizard_title_dynamic(&w), "inkworm — setup (4 / 4)");
        w.tts_enabled = true;
        assert_eq!(wizard_title_dynamic(&w), "inkworm — setup (4 / 7)");
    }

    #[test]
    fn mask_hides_tts_secrets() {
        assert_eq!(mask_for_display("secret", WizardStep::TtsApiKey), "******");
        assert_eq!(
            mask_for_display("secret", WizardStep::TtsApiSecret),
            "******"
        );
        assert_eq!(mask_for_display("app123", WizardStep::TtsAppId), "app123");
    }

    #[test]
    fn hint_tts_steps() {
        assert_eq!(
            wizard_hint(WizardStep::TtsEnable, WizardOrigin::FirstRun, false),
            "Enter · next     Esc · back"
        );
        assert_eq!(
            wizard_hint(WizardStep::TtsApiSecret, WizardOrigin::FirstRun, false),
            "Enter · test and save     Esc · back"
        );
    }

    #[test]
    fn hint_on_endpoint_firstrun_says_ctrl_c_quit() {
        let h = wizard_hint(WizardStep::Endpoint, WizardOrigin::FirstRun, false);
        assert!(h.contains("Ctrl+C"));
        assert!(!h.contains("back"));
    }

    #[test]
    fn hint_on_endpoint_command_says_esc_cancel() {
        let h = wizard_hint(WizardStep::Endpoint, WizardOrigin::Command, false);
        assert!(h.contains("Esc"));
        assert!(h.contains("cancel"));
    }

    #[test]
    fn hint_during_testing_mentions_connectivity() {
        let h = wizard_hint(WizardStep::Model, WizardOrigin::FirstRun, true);
        assert!(h.to_lowercase().contains("testing"));
    }
}
