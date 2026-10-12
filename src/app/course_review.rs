//! Daily review of the oldest overdue mastered courses, after mistakes.

use super::{App, Screen};
use crate::storage::course::list_courses;
use crate::storage::progress::{course_full_stats, CourseReview, OVER_LEARNED_THRESHOLD};
use crate::ui::study::{StudyMode, StudyPhase, StudyState};

impl App {
    pub(super) fn course_review_pending(&self) -> bool {
        self.study
            .progress()
            .course_review
            .as_ref()
            .and_then(CourseReview::current_id)
            .is_some()
    }

    fn in_course_review(&self) -> bool {
        matches!(self.study.mode(), StudyMode::Course)
            && self.course_review_pending()
            && self.study.current_course().map(|c| c.id.as_str())
                == self
                    .study
                    .progress()
                    .course_review
                    .as_ref()
                    .and_then(CourseReview::current_id)
    }

    /// Returns true when a review course takes over the study screen.
    pub(super) fn start_course_review(&mut self) -> bool {
        if matches!(self.study.mode(), StudyMode::Mistakes)
            || self.mistakes.peek_current_drill().is_some()
        {
            return false;
        }
        let today = self.clock.today_local();
        let review = self.study.progress().course_review.as_ref();
        if !self.course_review_pending() {
            if review.is_some_and(|r| r.date == today) {
                return false;
            }
            let cutoff = self.clock.now() - chrono::Duration::days(30);
            let Ok(metas) = list_courses(&self.data_paths.courses_dir) else {
                return false;
            };
            let mut candidates: Vec<_> = metas
                .into_iter()
                .filter_map(|meta| {
                    let cp = self.study.progress().course(&meta.id)?;
                    (cp.completion_count >= OVER_LEARNED_THRESHOLD && cp.last_studied_at < cutoff)
                        .then_some((cp.last_studied_at, meta.id))
                })
                .collect();
            candidates.sort();
            let queue: Vec<_> = candidates
                .into_iter()
                .filter(|(_, id)| self.load_course_owned(id).is_some())
                .take(3)
                .map(|(_, id)| id)
                .collect();
            if queue.is_empty() {
                return false;
            }
            self.study.progress_mut().course_review = Some(CourseReview {
                date: today,
                queue,
                next_index: 0,
                current_started: false,
            });
            self.mark_progress_dirty();
        }

        while let Some((id, started)) = self
            .study
            .progress()
            .course_review
            .as_ref()
            .and_then(|r| Some((r.current_id()?.to_owned(), r.current_started)))
        {
            let Some(course) = self.load_course_owned(&id) else {
                // Deleted or corrupt courses must not trap the user in review.
                self.advance_course_review_cursor();
                continue;
            };
            if !started {
                self.study.progress_mut().reset_course_full_drills(&course);
                self.study
                    .progress_mut()
                    .course_review
                    .as_mut()
                    .unwrap()
                    .current_started = true;
            } else {
                let stats = course_full_stats(&course, self.study.progress().course(&id));
                if stats.total_drills > 0 && stats.completed_drills == stats.total_drills {
                    // Restart after the last correct answer, before its advance tick.
                    let cp = self.study.progress_mut().course_mut(&id);
                    cp.completion_count = cp.completion_count.saturating_add(1);
                    self.advance_course_review_cursor();
                    continue;
                }
            }
            self.study.progress_mut().active_course_id = Some(id);
            self.study = StudyState::new(Some(course), self.study.progress().clone());
            self.screen = Screen::Study;
            self.course_list = None;
            self.complete_auto_switch_started_at = None;
            self.complete_auto_switch_cancelled = false;
            self.mark_progress_dirty();
            self.flush_all_now();
            self.spawn_bundle_prewarm();
            self.speak_current_drill();
            return true;
        }
        self.flush_all_now();
        false
    }

    fn advance_course_review_cursor(&mut self) {
        let today = self.clock.today_local();
        if let Some(review) = self.study.progress_mut().course_review.as_mut() {
            review.next_index += 1;
            review.current_started = false;
            // Finishing a carried-over batch also satisfies today's quota.
            review.date = today;
        }
        self.mark_progress_dirty();
    }

    pub(super) fn finish_course_review_if_complete(&mut self) -> bool {
        if !self.in_course_review() || !matches!(self.study.phase(), StudyPhase::Complete) {
            return false;
        }
        self.advance_course_review_cursor();
        if !self.start_course_review() {
            self.flush_all_now();
            self.set_toast("Daily course review complete ✓".into());
            self.open_course_list();
        }
        true
    }

    pub(super) fn show_course_review_required(&mut self) {
        self.set_toast("Finish daily course review first · Ctrl+C to save and quit".into());
    }
}
