//! Shared test helpers. Files in `tests/subdir/mod.rs` are NOT compiled as
//! separate test binaries — this is cargo's convention — so this module is
//! used by each top-level `tests/*.rs` via `mod common;`.

#![allow(dead_code)]

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
