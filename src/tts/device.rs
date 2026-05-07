//! Audio output device detection and TTS auto-mode decision.
//!
//! Classification + `should_speak` are pure (spec §7.5). `detect_output_kind`
//! shells out on macOS and returns `Unknown` gracefully when no detection
//! tool is available.

use std::io;
use std::process::Command;

use crate::config::TtsOverride;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Bluetooth,
    WiredHeadphones,
    BuiltInSpeaker,
    ExternalSpeaker,
    Unknown,
}

/// Classify a raw device name (case-insensitive).
/// Order-sensitive: more-specific rules come first.
pub fn classify(name: &str) -> OutputKind {
    let lower = name.to_lowercase();
    if lower.contains("airpods") || lower.contains("bluetooth") || lower.contains("beats") {
        return OutputKind::Bluetooth;
    }
    if lower.contains("headphone") || lower.contains("earphone") || lower.contains("headset") {
        return OutputKind::WiredHeadphones;
    }
    if lower.contains("macbook") && lower.contains("speaker") {
        return OutputKind::BuiltInSpeaker;
    }
    if lower.contains("display") || lower.contains("hdmi") {
        return OutputKind::ExternalSpeaker;
    }
    OutputKind::Unknown
}

/// Device + mode gate, creds-agnostic. Returns whether the audio output
/// is suitable AND the user hasn't explicitly turned playback off.
/// Used by both bundle and TTS paths uniformly.
pub fn should_play_bundle(mode: TtsOverride, device: OutputKind) -> bool {
    match mode {
        TtsOverride::On => true,
        TtsOverride::Off => false,
        TtsOverride::Auto => matches!(device, OutputKind::Bluetooth | OutputKind::WiredHeadphones),
    }
}

/// Whether TTS should play for the given mode/device/creds combination (spec §7.5).
pub fn should_speak(mode: TtsOverride, device: OutputKind, has_creds: bool) -> bool {
    has_creds && should_play_bundle(mode, device)
}

/// Best-effort audio-output detection. Returns `Unknown` on any failure or
/// unrecognized device name. On non-macOS systems where neither tool exists,
/// also returns `Unknown` (safe default: Auto mode won't trigger TTS).
pub fn detect_output_kind() -> io::Result<OutputKind> {
    // Try CPAL first (most reliable, works with Bluetooth devices)
    if let Some(name) = try_cpal_device()? {
        return Ok(classify(&name));
    }
    if let Some(name) = try_switchaudiosource()? {
        return Ok(classify(&name));
    }
    if let Some(name) = try_system_profiler()? {
        return Ok(classify(&name));
    }
    Ok(OutputKind::Unknown)
}

fn try_cpal_device() -> io::Result<Option<String>> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Some(device) = host.default_output_device() {
        if let Ok(name) = device.name() {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn try_switchaudiosource() -> io::Result<Option<String>> {
    let output = match Command::new("SwitchAudioSource")
        .args(["-c", "-t", "output"])
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

fn try_system_profiler() -> io::Result<Option<String>> {
    let output = match Command::new("system_profiler")
        .arg("SPAudioDataType")
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Walk lines; track the most recent 4-space-indented device name heading.
    // When "Default Output Device: Yes" or "Output Source: Default" appears under it, return that heading.
    let mut last_heading: Option<String> = None;
    for line in text.lines() {
        if line.starts_with("    ") && !line.starts_with("     ") && line.trim_end().ends_with(':')
        {
            let name = line.trim().trim_end_matches(':').to_string();
            if !name.is_empty() {
                last_heading = Some(name);
            }
        }
        if line.contains("Default Output Device: Yes") || line.contains("Output Source: Default") {
            if let Some(name) = last_heading.take() {
                return Ok(Some(name));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airpods_classify_as_bluetooth() {
        assert_eq!(classify("AirPods Pro"), OutputKind::Bluetooth);
        assert_eq!(classify("airpods max"), OutputKind::Bluetooth);
    }

    #[test]
    fn bluetooth_keyword_classifies_as_bluetooth() {
        assert_eq!(classify("Generic Bluetooth Speaker"), OutputKind::Bluetooth);
    }

    #[test]
    fn beats_classify_as_bluetooth() {
        assert_eq!(classify("Beats Studio3"), OutputKind::Bluetooth);
    }

    #[test]
    fn headphones_classify_as_wired() {
        assert_eq!(classify("External Headphones"), OutputKind::WiredHeadphones);
        assert_eq!(classify("USB Headset"), OutputKind::WiredHeadphones);
        assert_eq!(classify("Earphone"), OutputKind::WiredHeadphones);
    }

    #[test]
    fn macbook_speaker_classifies_as_builtin() {
        assert_eq!(classify("MacBook Pro Speakers"), OutputKind::BuiltInSpeaker);
    }

    #[test]
    fn display_and_hdmi_classify_as_external() {
        assert_eq!(
            classify("LG UltraFine Display Audio"),
            OutputKind::ExternalSpeaker
        );
        assert_eq!(classify("HDMI Output"), OutputKind::ExternalSpeaker);
    }

    #[test]
    fn unknown_falls_through() {
        assert_eq!(classify("Mystery Device"), OutputKind::Unknown);
        assert_eq!(classify(""), OutputKind::Unknown);
    }

    #[test]
    fn case_insensitive_matching() {
        assert_eq!(classify("AIRPODS"), OutputKind::Bluetooth);
        assert_eq!(classify("HeadPhones"), OutputKind::WiredHeadphones);
    }

    #[test]
    fn should_speak_off_always_silent() {
        assert!(!should_speak(TtsOverride::Off, OutputKind::Bluetooth, true));
        assert!(!should_speak(
            TtsOverride::Off,
            OutputKind::WiredHeadphones,
            true
        ));
    }

    #[test]
    fn should_speak_on_plays_if_creds() {
        assert!(should_speak(
            TtsOverride::On,
            OutputKind::BuiltInSpeaker,
            true
        ));
        assert!(should_speak(TtsOverride::On, OutputKind::Unknown, true));
        assert!(!should_speak(TtsOverride::On, OutputKind::Bluetooth, false));
    }

    #[test]
    fn should_speak_auto_plays_only_on_headphones() {
        assert!(should_speak(TtsOverride::Auto, OutputKind::Bluetooth, true));
        assert!(should_speak(
            TtsOverride::Auto,
            OutputKind::WiredHeadphones,
            true
        ));
        assert!(!should_speak(
            TtsOverride::Auto,
            OutputKind::BuiltInSpeaker,
            true
        ));
        assert!(!should_speak(
            TtsOverride::Auto,
            OutputKind::ExternalSpeaker,
            true
        ));
        assert!(!should_speak(TtsOverride::Auto, OutputKind::Unknown, true));
    }

    #[test]
    fn detect_returns_unknown_when_tools_missing_or_fail() {
        let result = detect_output_kind();
        assert!(
            result.is_ok(),
            "should never propagate an io error: {result:?}"
        );
    }
}
