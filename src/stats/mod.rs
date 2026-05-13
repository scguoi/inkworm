//! Learning statistics — pure in-memory tracking and aggregation.
//!
//! See spec: docs/superpowers/specs/2026-05-13-learning-stats-design.md

pub mod tracker;

pub use tracker::{StatsTracker, IDLE_THRESHOLD_MS};
