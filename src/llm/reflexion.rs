//! Two-phase Reflexion-style course generator.
//!
//! Phase 1: split an article into 5–20 (chinese, english) sentence pairs.
//! Phase 2: expand each sentence into 3–5 progressive drills (concurrent,
//!          bounded by `max_concurrent_calls`).
//! Each LLM call has up to 3 repair attempts; validation errors are fed back
//! to the model as a repair prompt. On total failure, the raw response chain
//! is written to `paths.failed_dir`.

use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::clock::Clock;
use crate::llm::client::LlmClient;
use crate::llm::error::LlmError;
use crate::llm::prompt::{repair_message, PHASE1_SYSTEM};
use crate::llm::types::{ChatRequest, RawSentences};
use crate::storage::failed::{save_failed_response, AttemptFailure};
use crate::storage::DataPaths;

/// Errors that can end a Reflexion run.
#[derive(Debug, thiserror::Error)]
pub enum ReflexionError {
    /// Three attempts in a row failed validation. Raw responses were saved.
    #[error("phase {phase} attempts exhausted; saved to {saved_to:?}")]
    AllAttemptsFailed {
        phase: u8,
        sentence_index: Option<usize>,
        saved_to: PathBuf,
        last_attempts: Vec<AttemptFailure>,
    },
    /// An LLM transport error (network/auth/5xx) — not counted against retry.
    #[error("llm: {0}")]
    Llm(#[from] LlmError),
    /// Caller cancelled via the CancellationToken.
    #[error("cancelled")]
    Cancelled,
    /// Total budget exceeded.
    #[error("budget exceeded")]
    BudgetExceeded,
    /// Storage failure when writing a failed/ report.
    #[error("storage: {0}")]
    Storage(#[from] crate::storage::StorageError),
}

/// Orchestrates one invocation of the two-phase generator.
pub struct Reflexion<'a> {
    pub client: &'a dyn LlmClient,
    pub clock: &'a dyn Clock,
    pub paths: &'a DataPaths,
    pub model: &'a str,
    pub max_concurrent: usize,
    pub cancel: CancellationToken,
}

impl<'a> Reflexion<'a> {
    /// Phase 1: one LLM call (with up to 3 repairs) producing a `RawSentences`.
    pub async fn reflexion_split(&self, article: &str) -> Result<RawSentences, ReflexionError> {
        let user_prompt =
            format!("Article to split:\n\"\"\"\n{article}\n\"\"\"\n\nReturn JSON only.");
        let mut req = ChatRequest::system_and_user(
            self.model.to_string(),
            PHASE1_SYSTEM.to_string(),
            user_prompt.clone(),
        );
        let mut failures: Vec<AttemptFailure> = Vec::new();

        for attempt in 1..=3u32 {
            if self.cancel.is_cancelled() {
                return Err(ReflexionError::Cancelled);
            }
            let raw = self.client.chat(req.clone(), self.cancel.clone()).await?;
            match try_parse_and_validate_phase1(&raw) {
                Ok(rs) => return Ok(rs),
                Err(errors) => {
                    failures.push(AttemptFailure {
                        attempt_number: attempt,
                        raw: raw.clone(),
                        errors: errors.clone(),
                    });
                    if attempt == 3 {
                        let path = save_failed_response(
                            &self.paths.failed_dir,
                            self.clock.now(),
                            1,
                            None,
                            self.model,
                            article,
                            &failures,
                        )?;
                        return Err(ReflexionError::AllAttemptsFailed {
                            phase: 1,
                            sentence_index: None,
                            saved_to: path,
                            last_attempts: failures,
                        });
                    }
                    req.append_repair(raw, repair_message(&errors));
                }
            }
        }
        unreachable!("loop returns on attempt == 3")
    }
}

/// Try to parse the raw string as `RawSentences` and validate it. Returns the
/// flat list of error strings on failure, or `Ok(RawSentences)` on success.
fn try_parse_and_validate_phase1(raw: &str) -> Result<RawSentences, Vec<String>> {
    let parsed: RawSentences = match serde_json::from_str(strip_code_fences(raw)) {
        Ok(p) => p,
        Err(e) => return Err(vec![format!("JSON parse failed: {e}")]),
    };
    let errs = parsed.validate();
    if errs.is_empty() {
        Ok(parsed)
    } else {
        Err(errs)
    }
}

use std::sync::Arc;

use futures::future::try_join_all;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::llm::prompt::PHASE2_SYSTEM;
use crate::llm::types::{RawDrill, RawDrills, RawSentence};

impl<'a> Reflexion<'a> {
    /// Phase 2: expand ONE sentence into drills via LLM. Up to 3 repair attempts.
    /// Returns `Ok(RawDrills)` or a `ReflexionError::AllAttemptsFailed { phase: 2 }`.
    pub async fn reflexion_drill(
        &self,
        sentence_index: usize,
        sentence: &RawSentence,
    ) -> Result<RawDrills, ReflexionError> {
        let user_prompt = json!({
            "chinese": sentence.chinese,
            "english": sentence.english,
        })
        .to_string();
        let mut req = ChatRequest::system_and_user(
            self.model.to_string(),
            PHASE2_SYSTEM.to_string(),
            user_prompt.clone(),
        );
        let mut failures: Vec<AttemptFailure> = Vec::new();

        for attempt in 1..=3u32 {
            if self.cancel.is_cancelled() {
                return Err(ReflexionError::Cancelled);
            }
            let raw = self.client.chat(req.clone(), self.cancel.clone()).await?;
            match try_parse_and_validate_phase2(&raw, &sentence.english) {
                Ok(rd) => return Ok(rd),
                Err(errors) => {
                    failures.push(AttemptFailure {
                        attempt_number: attempt,
                        raw: raw.clone(),
                        errors: errors.clone(),
                    });
                    if attempt == 3 {
                        let path = save_failed_response(
                            &self.paths.failed_dir,
                            self.clock.now(),
                            2,
                            Some(sentence_index),
                            self.model,
                            &sentence.english,
                            &failures,
                        )?;
                        return Err(ReflexionError::AllAttemptsFailed {
                            phase: 2,
                            sentence_index: Some(sentence_index),
                            saved_to: path,
                            last_attempts: failures,
                        });
                    }
                    req.append_repair(raw, repair_message(&errors));
                }
            }
        }
        unreachable!("loop returns on attempt == 3")
    }

    /// Run `reflexion_drill` for each sentence, bounded by `max_concurrent`.
    /// Any single-sentence failure returns an error and cancels the rest.
    pub async fn orchestrate_phase2(
        &self,
        sentences: &[RawSentence],
    ) -> Result<Vec<RawDrills>, ReflexionError> {
        let sem = Arc::new(Semaphore::new(self.max_concurrent.max(1)));
        let tasks: Vec<_> = sentences
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let sem = sem.clone();
                let sentence = s.clone();
                async move {
                    let _permit = sem.acquire().await.unwrap();
                    self.reflexion_drill(i, &sentence).await
                }
            })
            .collect();
        try_join_all(tasks).await
    }
}

/// Try to parse the raw string as `RawDrills` and validate it against the
/// reference english. Returns the flat error list on failure.
fn try_parse_and_validate_phase2(
    raw: &str,
    reference_english: &str,
) -> Result<RawDrills, Vec<String>> {
    let parsed: RawDrills = match serde_json::from_str(strip_code_fences(raw)) {
        Ok(p) => p,
        Err(e) => return Err(vec![format!("JSON parse failed: {e}")]),
    };
    let errs = parsed.validate(reference_english);
    if errs.is_empty() {
        Ok(parsed)
    } else {
        Err(errs)
    }
}

// Temporarily silence `unused import` for RawDrill until Task 8.1 uses it.
#[allow(dead_code)]
fn __keep_raw_drill_in_scope(_: RawDrill) {}

/// Tolerate LLMs that wrap JSON in ```json ... ``` despite being told not to.
fn strip_code_fences(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```json") {
        rest.trim_start()
            .strip_suffix("```")
            .map(str::trim)
            .unwrap_or(rest)
    } else if let Some(rest) = t.strip_prefix("```") {
        rest.trim_start()
            .strip_suffix("```")
            .map(str::trim)
            .unwrap_or(rest)
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_code_fences_removes_json_fences() {
        assert_eq!(strip_code_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("{\"a\":1}"), "{\"a\":1}");
    }
}
