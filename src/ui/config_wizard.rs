//! Config wizard: 3-step first-run / `/config` flow for LLM endpoint / api_key / model.
//! Connectivity probe and rendering live in this file (probe in §Probe block, render in §Render block).

use tokio_util::sync::CancellationToken;

use crate::config::Config;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
