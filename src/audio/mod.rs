//! Bundled course audio: path resolution + mp3 playback.
//!
//! Independent of `crate::tts` — bundles never hit the network and never
//! share the ElevenLabs mp3 cache. See
//! `docs/superpowers/specs/2026-05-07-bundled-course-audio-design.md`.

pub mod bundle;
pub mod player;
