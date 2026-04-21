use crate::error::AppError;
use crate::storage::course::Course;

/// Messages sent from background tasks to the main event loop.
#[derive(Debug)]
pub enum TaskMsg {
    Generate(GenerateProgress),
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
