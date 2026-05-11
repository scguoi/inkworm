use crate::error::AppError;
use crate::storage::course::Course;
use crate::tts::OutputKind;

/// Messages sent from background tasks to the main event loop.
#[derive(Debug)]
pub enum TaskMsg {
    Generate(GenerateProgress),
    Wizard(WizardTaskMsg),
    DeviceDetected(OutputKind),
    TtsSpeakResult(Result<(), TtsSpeakErr>),
    /// Prewarm of bundled-course audio has made progress.
    /// `done` is the count of files processed (succeeded + failed) so far;
    /// `total` is the placeholder count when the run started.
    PrewarmProgress {
        generation: u64,
        done: u32,
        total: u32,
    },
    /// Prewarm finished (or was cancelled by a newer generation).
    PrewarmDone {
        generation: u64,
        ok: u32,
        failed: u32,
    },
}

/// Failure carried back from a background `speak` task.
/// `is_auth` is true when the underlying `TtsError` was `Auth` — those failures
/// won't self-heal and trigger immediate session disable instead of counting.
#[derive(Debug, Clone)]
pub struct TtsSpeakErr {
    pub message: String,
    pub is_auth: bool,
}

/// Progress updates from the Generate background task.
#[derive(Debug)]
pub enum GenerateProgress {
    Phase1Started,
    Phase1Done { sentence_count: usize },
    Phase2Progress { done: usize, total: usize },
    Done(Course),
    Failed(AppError),
}

/// Result from the ConfigWizard connectivity probe.
#[derive(Debug)]
pub enum WizardTaskMsg {
    ConnectivityOk,
    ConnectivityFailed(AppError),
    TtsProbeOk,
    TtsProbeFailed(AppError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prewarm_variants_construct_as_specified() {
        let progress = TaskMsg::PrewarmProgress {
            generation: 1,
            done: 5,
            total: 10,
        };
        let done = TaskMsg::PrewarmDone {
            generation: 1,
            ok: 9,
            failed: 1,
        };
        // Smoke: Debug derive still works after adding variants.
        let _ = format!("{:?}", progress);
        let _ = format!("{:?}", done);
    }
}
