//! Shared test helpers. Files under `tests/common/` are not compiled as
//! separate integration binaries — this module is used by each top-level
//! `tests/*.rs` via `mod common;`.

#![allow(dead_code)]

pub mod llm_mocks;

use std::path::PathBuf;
use tempfile::TempDir;

pub struct TestEnv {
    pub _tmp: TempDir,
    pub home: PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let home = tmp.path().to_path_buf();
        Self { _tmp: tmp, home }
    }
}

use inkworm::storage::course::Course;

pub fn load_minimal_course() -> Course {
    let json = include_str!("../../fixtures/courses/good/minimal.json");
    serde_json::from_str(json).unwrap()
}
