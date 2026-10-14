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
        self.prepare_course_review();

        while let Some((id, started)) = self
            .study
            .progress()
            .course_review
            .as_ref()
            .and_then(|r| Some((r.current_id()?.to_owned(), r.current_started)))
        {
            let Some(course) = self.load_course_owned(&id) else {
                // Deleted or corrupt courses must not trap the user in review.
                self.advance_course_review_cursor(false);
                self.prepare_course_review();
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
                    self.advance_course_review_cursor(true);
                    self.prepare_course_review();
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

    fn normalize_course_review_date(&mut self) {
        let today = self.clock.today_local();
        let progress = self.study.progress_mut();
        let Some(review) = progress.course_review.as_mut() else {
            return;
        };
        if review.date == today && review.completed_today.is_some() {
            return;
        }
        if review.completed_today.is_none() {
            // Recover legacy batches, including batches completed across two days.
            review.completed_today = Some(
                review
                    .queue
                    .iter()
                    .take(review.next_index)
                    .filter(|id| {
                        progress.courses.get(*id).is_some_and(|cp| {
                            cp.last_studied_at
                                .with_timezone(&chrono::Local)
                                .date_naive()
                                == today
                        })
                    })
                    .count(),
            );
        } else if review.date != today {
            review.completed_today = Some(0);
        }
        if review.date != today {
            review.queue.drain(..review.next_index);
            review.next_index = 0;
            review.date = today;
        }
        self.mark_progress_dirty();
    }

    fn prepare_course_review(&mut self) {
        self.normalize_course_review_date();
        let review = self.study.progress().course_review.as_ref();
        let completed = review.and_then(|r| r.completed_today).unwrap_or(0);
        let pending = review.map_or(0, |r| r.queue.len() - r.next_index);
        let needed = 3usize.saturating_sub(completed + pending);
        if needed == 0 {
            return;
        }
        let cutoff = self.clock.now() - chrono::Duration::days(30);
        let Ok(metas) = list_courses(&self.data_paths.courses_dir) else {
            return;
        };
        let mut candidates: Vec<_> = metas
            .into_iter()
            .filter_map(|meta| {
                let cp = self.study.progress().course(&meta.id)?;
                (cp.completion_count >= OVER_LEARNED_THRESHOLD
                    && cp.last_studied_at < cutoff
                    && !review.is_some_and(|r| r.queue.contains(&meta.id)))
                .then_some((cp.last_studied_at, meta.id))
            })
            .collect();
        candidates.sort();
        let additions: Vec<_> = candidates
            .into_iter()
            .filter(|(_, id)| self.load_course_owned(id).is_some())
            .take(needed)
            .map(|(_, id)| id)
            .collect();
        if additions.is_empty() {
            return;
        }
        let today = self.clock.today_local();
        let review = self
            .study
            .progress_mut()
            .course_review
            .get_or_insert_with(|| CourseReview {
                date: today,
                queue: Vec::new(),
                next_index: 0,
                current_started: false,
                completed_today: Some(0),
            });
        review.queue.extend(additions);
        self.mark_progress_dirty();
    }

    fn advance_course_review_cursor(&mut self, completed: bool) {
        self.normalize_course_review_date();
        let today = self.clock.today_local();
        let completed_today = completed
            && self
                .study
                .progress()
                .course_review
                .as_ref()
                .and_then(CourseReview::current_id)
                .and_then(|id| self.study.progress().course(id))
                .is_some_and(|cp| {
                    cp.last_studied_at
                        .with_timezone(&chrono::Local)
                        .date_naive()
                        == today
                });
        if let Some(review) = self.study.progress_mut().course_review.as_mut() {
            review.next_index += 1;
            review.current_started = false;
            if completed_today {
                *review.completed_today.get_or_insert(0) += 1;
            }
        }
        self.mark_progress_dirty();
    }

    pub(super) fn finish_course_review_if_complete(&mut self) -> bool {
        if !self.in_course_review() || !matches!(self.study.phase(), StudyPhase::Complete) {
            return false;
        }
        self.advance_course_review_cursor(true);
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
