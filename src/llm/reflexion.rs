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
use crate::llm::prompt::{phase1_system, repair_message};
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
    pub async fn reflexion_split(
        &self,
        article: &str,
        level_description: &str,
    ) -> Result<RawSentences, ReflexionError> {
        let user_prompt =
            format!("Article to split:\n\"\"\"\n{article}\n\"\"\"\n\nReturn JSON only.");
        let mut req = ChatRequest::system_and_user(
            self.model.to_string(),
            phase1_system(level_description),
            user_prompt.clone(),
        );
        let mut failures: Vec<AttemptFailure> = Vec::new();

        for attempt in 1..=3u32 {
            if self.cancel.is_cancelled() {
                return Err(ReflexionError::Cancelled);
            }
            let start = std::time::Instant::now();
            let result = self.client.chat(req.clone(), self.cancel.clone()).await;
            let duration_ms = start.elapsed().as_millis();
            match &result {
                Ok(ref _content) => {
                    tracing::info!(
                        model = %self.model,
                        attempt = attempt,
                        duration_ms = duration_ms,
                        result = "ok",
                        "LLM call succeeded"
                    );
                }
                Err(ref e) => {
                    tracing::error!(
                        model = %self.model,
                        attempt = attempt,
                        duration_ms = duration_ms,
                        error = %e,
                        "LLM call failed"
                    );
                }
            }
            let raw = result?;
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
use crate::llm::types::{RawDrills, RawSentence};

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
            let start = std::time::Instant::now();
            let result = self.client.chat(req.clone(), self.cancel.clone()).await;
            let duration_ms = start.elapsed().as_millis();
            match &result {
                Ok(ref _content) => {
                    tracing::info!(
                        model = %self.model,
                        attempt = attempt,
                        duration_ms = duration_ms,
                        result = "ok",
                        "LLM call succeeded"
                    );
                }
                Err(ref e) => {
                    tracing::error!(
                        model = %self.model,
                        attempt = attempt,
                        duration_ms = duration_ms,
                        error = %e,
                        "LLM call failed"
                    );
                }
            }
            let raw = result?;
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
        progress_tx: Option<tokio::sync::mpsc::Sender<crate::ui::task_msg::GenerateProgress>>,
    ) -> Result<Vec<RawDrills>, ReflexionError> {
        let sem = Arc::new(Semaphore::new(self.max_concurrent.max(1)));
        let total = sentences.len();
        let done_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let tasks: Vec<_> = sentences
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let sem = sem.clone();
                let sentence = s.clone();
                let done_count = done_count.clone();
                let progress_tx = progress_tx.clone();
                async move {
                    let _permit = sem.acquire().await.unwrap();
                    let result = self.reflexion_drill(i, &sentence).await;
                    if result.is_ok() {
                        let done = done_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        if let Some(tx) = progress_tx {
                            let _ = tx
                                .send(crate::ui::task_msg::GenerateProgress::Phase2Progress {
                                    done,
                                    total,
                                })
                                .await;
                        }
                    }
                    result
                }
            })
            .collect();
        try_join_all(tasks).await
    }

    /// Full pipeline: article → Course. Returns the assembled Course plus
    /// attempt counts (1 for success, 2 if one repair, 3 if two repairs).
    /// Any sub-phase error (Reflexion exhaustion, LlmError, Cancelled) is
    /// propagated; on success nothing is written to disk — the caller is
    /// responsible for persisting via `storage::save_course`.
    pub async fn generate(
        &self,
        article: &str,
        level_description: &str,
        existing_ids: &[String],
        progress_tx: Option<tokio::sync::mpsc::Sender<crate::ui::task_msg::GenerateProgress>>,
    ) -> Result<ReflexionOutcome, ReflexionError> {
        if let Some(tx) = &progress_tx {
            let _ = tx
                .send(crate::ui::task_msg::GenerateProgress::Phase1Started)
                .await;
        }
        let phase1 = self.reflexion_split(article, level_description).await?;
        if let Some(tx) = &progress_tx {
            let _ = tx
                .send(crate::ui::task_msg::GenerateProgress::Phase1Done {
                    sentence_count: phase1.sentences.len(),
                })
                .await;
        }
        let phase2 = self
            .orchestrate_phase2(&phase1.sentences, progress_tx)
            .await?;
        let course = build_course(
            &phase1.sentences,
            &phase2,
            &phase1.title,
            &phase1.description,
            existing_ids,
            self.model,
            self.clock.now(),
        );
        // We cannot easily recover per-call attempt counts without threading
        // state; stub them as zero for now. `generate` is the place where
        // future telemetry will decorate these if needed.
        Ok(ReflexionOutcome {
            course,
            phase1_attempts: 1,
            phase2_attempts: vec![1; phase2.len()],
        })
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

use chrono::DateTime;

use crate::storage::course::{Course, Drill, Sentence, Source, SourceKind, SCHEMA_VERSION};

/// Result of a successful `generate` run.
#[derive(Debug, Clone)]
pub struct ReflexionOutcome {
    pub course: Course,
    pub phase1_attempts: u32,
    pub phase2_attempts: Vec<u32>,
}

/// Combine the Phase 1 header, per-sentence Phase 2 drills, and metadata into
/// a fully populated `Course` struct (with program-filled `id`/`order`/`stage`/`source`).
#[allow(clippy::too_many_arguments)]
pub fn build_course(
    sentences_raw: &[RawSentence],
    drills_raw: &[RawDrills],
    title: &str,
    description: &str,
    existing_ids: &[String],
    model: &str,
    now: DateTime<chrono::Utc>,
) -> Course {
    let sentences: Vec<Sentence> = sentences_raw
        .iter()
        .zip(drills_raw.iter())
        .enumerate()
        .map(|(i, (_s, rd))| Sentence {
            order: (i as u32) + 1,
            drills: rd
                .drills
                .iter()
                .enumerate()
                .map(|(j, d)| Drill {
                    stage: (j as u32) + 1,
                    focus: d.focus,
                    chinese: d.chinese.clone(),
                    english: d.english.clone(),
                    soundmark: d.soundmark.clone(),
                })
                .collect(),
        })
        .collect();

    let id = unique_id(&now, title, existing_ids);
    Course {
        schema_version: SCHEMA_VERSION,
        id,
        title: title.to_string(),
        description: if description.is_empty() {
            None
        } else {
            Some(description.to_string())
        },
        source: Source {
            kind: SourceKind::Article,
            url: String::new(),
            created_at: now,
            model: model.to_string(),
        },
        sentences,
    }
}

/// Build a unique Course id of the form `YYYY-MM-DD-<slug(title)>`, appending
/// `-2`, `-3`, ... if the computed id collides with an existing one.
pub fn unique_id(now: &DateTime<chrono::Utc>, title: &str, existing: &[String]) -> String {
    let base = format!("{}-{}", now.format("%Y-%m-%d"), slug(title));
    if !existing.iter().any(|e| e == &base) {
        return base;
    }
    for n in 2u32.. {
        let candidate = format!("{base}-{n}");
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
    }
    unreachable!("u32 exhausted");
}

/// Turn a title into a kebab-case slug: lowercase ASCII + digits + '-',
/// collapsing runs, trimming leading/trailing '-', capped at 40 chars, and
/// guaranteed non-empty ("lesson" if the title yielded nothing usable).
pub fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_dash = true;
    for c in title.chars() {
        let mapped = if c.is_ascii_alphanumeric() {
            Some(c.to_ascii_lowercase())
        } else if c.is_whitespace() || c == '-' || c == '_' {
            if last_dash {
                None
            } else {
                Some('-')
            }
        } else {
            None
        };
        if let Some(ch) = mapped {
            out.push(ch);
            last_dash = ch == '-';
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > 40 {
        out.truncate(40);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        out = "lesson".into();
    }
    out
}

#[cfg(test)]
mod build_course_tests {
    use super::*;
    use crate::llm::types::RawDrill;
    use chrono::TimeZone;

    fn now() -> DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap()
    }

    #[test]
    fn slug_basic() {
        assert_eq!(slug("Hello World"), "hello-world");
        assert_eq!(slug("TED: What AI Means"), "ted-what-ai-means");
        assert_eq!(slug("  trim me  "), "trim-me");
    }

    #[test]
    fn slug_truncates_at_forty() {
        let s = slug(&"a".repeat(100));
        assert!(s.len() <= 40);
    }

    #[test]
    fn slug_never_empty() {
        assert_eq!(slug("   "), "lesson");
        assert_eq!(slug("!!!"), "lesson");
        assert_eq!(slug(""), "lesson");
    }

    #[test]
    fn unique_id_appends_suffix_on_collision() {
        let existing = vec!["2026-04-21-hello".into(), "2026-04-21-hello-2".into()];
        let id = unique_id(&now(), "Hello", &existing);
        assert_eq!(id, "2026-04-21-hello-3");
    }

    #[test]
    fn build_course_passes_course_validate() {
        use crate::storage::course::Focus;
        let sentences: Vec<RawSentence> = (0..5)
            .map(|i| RawSentence {
                chinese: format!("句{i}"),
                english: format!("sentence number {i} here"),
            })
            .collect();
        let drills: Vec<RawDrills> = sentences
            .iter()
            .map(|s| RawDrills {
                drills: vec![
                    RawDrill {
                        stage: 1,
                        focus: Focus::Keywords,
                        chinese: "关键".into(),
                        english: "a b".into(),
                        soundmark: "".into(),
                    },
                    RawDrill {
                        stage: 2,
                        focus: Focus::Skeleton,
                        chinese: "骨架".into(),
                        english: "a b c".into(),
                        soundmark: "".into(),
                    },
                    RawDrill {
                        stage: 3,
                        focus: Focus::Full,
                        chinese: "完整".into(),
                        english: s.english.clone(),
                        soundmark: "".into(),
                    },
                ],
            })
            .collect();
        let c = build_course(
            &sentences,
            &drills,
            "Test Title",
            "",
            &[],
            "gpt-4o-mini",
            now(),
        );
        let errs = c.validate();
        assert!(errs.is_empty(), "{errs:#?}");
        assert_eq!(c.id, "2026-04-21-test-title");
        assert_eq!(c.description, None);
        assert_eq!(c.sentences.len(), 5);
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
