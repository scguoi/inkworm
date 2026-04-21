//! Config wizard: 3-step first-run / `/config` flow for LLM endpoint / api_key / model.
//! Connectivity probe and rendering live in this file (probe in §Probe block, render in §Render block).

use std::time::Duration;

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
}

/// Outcome of `WizardState::commit` — tells App what to do next.
#[derive(Debug)]
pub enum CommitOutcome {
    /// Advance to next step (input already seeded with draft value for the new step).
    Advance,
    /// On Model step — spawn connectivity test.
    ProbeConnectivity,
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
        Self {
            step: WizardStep::Endpoint,
            origin,
            draft,
            input,
            testing: None,
            error: None,
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
        if trimmed.is_empty() {
            let label = match self.step {
                WizardStep::Endpoint => "Endpoint cannot be empty",
                WizardStep::ApiKey => "API key cannot be empty",
                WizardStep::Model => "Model cannot be empty",
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
pub fn wizard_title(step: WizardStep) -> String {
    let n = match step {
        WizardStep::Endpoint => 1,
        WizardStep::ApiKey => 2,
        WizardStep::Model => 3,
    };
    format!("inkworm — setup ({n} / 3)")
}

/// Step-specific label.
pub fn wizard_step_label(step: WizardStep) -> &'static str {
    match step {
        WizardStep::Endpoint => "LLM endpoint",
        WizardStep::ApiKey => "LLM API key",
        WizardStep::Model => "LLM model",
    }
}

/// Display-ready input — masks the ApiKey step, passes through otherwise.
pub fn mask_for_display(input: &str, step: WizardStep) -> String {
    if step == WizardStep::ApiKey {
        "*".repeat(input.chars().count())
    } else {
        input.to_string()
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
            matches!(err, AppError::Llm(crate::llm::error::LlmError::Unauthorized)),
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
    fn title_shows_1_of_3_on_endpoint() {
        assert_eq!(wizard_title(WizardStep::Endpoint), "inkworm — setup (1 / 3)");
        assert_eq!(wizard_title(WizardStep::ApiKey), "inkworm — setup (2 / 3)");
        assert_eq!(wizard_title(WizardStep::Model), "inkworm — setup (3 / 3)");
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

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn render_config_wizard(frame: &mut Frame, state: &WizardState, cursor_visible: bool) {
    let area = frame.area();

    let title = wizard_title(state.step);
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
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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
