use std::path::PathBuf;
use std::sync::Arc;

use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::load_course;
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::tts::speaker::build_speaker;
use inkworm::ui::config_wizard::WizardOrigin;
use inkworm::ui::event::run_loop;
use inkworm::ui::terminal::{install_panic_hook, TerminalGuard};

fn init_tracing(log_dir: &std::path::Path) {
    let file_appender = tracing_appender::rolling::never(log_dir, "inkworm.log");
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("INKWORM_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(file_appender)
        .with_ansi(false)
        .init();
}

fn main() -> anyhow::Result<()> {
    if std::env::args()
        .nth(1)
        .is_some_and(|a| a == "--version" || a == "-V")
    {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    install_panic_hook();

    let cli_config: Option<PathBuf> = std::env::args()
        .nth(1)
        .filter(|a| a == "--config")
        .and_then(|_| std::env::args().nth(2))
        .map(PathBuf::from);

    let paths = DataPaths::resolve(cli_config.as_deref())?;
    paths.ensure_dirs()?;

    init_tracing(&paths.root);
    tracing::info!("inkworm starting");

    let (config, needs_wizard) = match Config::load(&paths.config_file) {
        Ok(c) if c.validate_llm().is_empty() => (c, false),
        Ok(c) => {
            for err in c.validate_llm() {
                eprintln!("config: {err}");
            }
            (c, true)
        }
        Err(e) => {
            eprintln!("config: could not load {:?}: {e}", paths.config_file);
            (Config::default(), true)
        }
    };

    let progress = Progress::load(&paths.progress_file)?;

    let mut boot_warnings: Vec<String> = Vec::new();

    let mut mistakes = match inkworm::storage::mistakes::MistakeBook::load(&paths.mistakes_file) {
        Ok(b) => b,
        Err(e) => {
            // Spec §6 row 2: rename corrupt file to .bak.{ts} and start empty.
            let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
            let bak = paths
                .mistakes_file
                .with_file_name(format!("mistakes.json.bak.{ts}"));
            let _ = std::fs::rename(&paths.mistakes_file, &bak);
            eprintln!(
                "mistakes: load failed ({e}); backed up to {} and starting empty",
                bak.display()
            );
            tracing::warn!(
                "mistakes: load failed ({e}); backed up to {} and starting empty",
                bak.display()
            );
            boot_warnings.push(format!("错题本损坏，已备份并从空开始（{}）", bak.display()));
            inkworm::storage::mistakes::MistakeBook::empty()
        }
    };

    // Defensive: drop entries pointing at courses/sentences/stages that no
    // longer exist (e.g. user manually deleted a course file, or an LLM
    // regenerated a course with different sentence/stage shape). Spec §3.4.
    let mut courses_for_prune: std::collections::HashMap<String, inkworm::storage::course::Course> =
        std::collections::HashMap::new();
    if let Ok(metas) = inkworm::storage::course::list_courses(&paths.courses_dir) {
        for meta in metas {
            if let Ok(c) = inkworm::storage::course::load_course(&paths.courses_dir, &meta.id) {
                courses_for_prune.insert(meta.id, c);
            }
        }
    }
    let entries_before = mistakes.entries.len();
    let wrong_streaks_before = mistakes.wrong_streaks.len();
    mistakes.prune_orphans(|id| courses_for_prune.get(id));
    let pruned_entries = entries_before.saturating_sub(mistakes.entries.len());
    let pruned_streaks = wrong_streaks_before.saturating_sub(mistakes.wrong_streaks.len());
    if pruned_entries + pruned_streaks > 0 {
        let msg = format!(
            "mistakes: pruned {} orphan entries and {} orphan streaks at startup",
            pruned_entries, pruned_streaks
        );
        eprintln!("{msg}");
        tracing::info!("{msg}");
        boot_warnings.push(format!(
            "已清理 {} 条孤立错题（课程缺失）",
            pruned_entries + pruned_streaks
        ));
        let _ = mistakes.save(&paths.mistakes_file);
    }

    let course = progress
        .active_course_id
        .as_deref()
        .and_then(|id| load_course(&paths.courses_dir, id).ok());

    // Try to open a rodio OutputStream once, up-front. `OutputStream` itself
    // is `!Send` and must stay alive on this (main) thread for audio to
    // continue playing. We pass its `OutputStreamHandle` (Send+Sync) into
    // the speaker. On failure we fall back to cache-only mode — the user
    // can still warm the cache via /tts on, but playback is disabled.
    let (_output_stream, audio_handle) = match rodio::OutputStream::try_default() {
        Ok((stream, handle)) => (Some(stream), Some(handle)),
        Err(e) => {
            eprintln!("TTS: audio device unavailable ({e}). Playback disabled.");
            (None, None)
        }
    };

    let speaker: Arc<dyn inkworm::tts::speaker::Speaker> = Arc::from(build_speaker(
        &config.tts.iflytek,
        paths.tts_cache_dir.clone(),
        config.tts.r#override,
        audio_handle,
    ));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let mut guard = TerminalGuard::new()?;
        let (task_tx, task_rx) = tokio::sync::mpsc::channel(32);
        let combined_boot_warning = if boot_warnings.is_empty() {
            None
        } else {
            Some(boot_warnings.join(" · "))
        };
        let mut app = App::new(
            course,
            progress,
            paths,
            Arc::new(SystemClock),
            config,
            mistakes,
            combined_boot_warning,
            task_tx,
            speaker,
        );

        // Detect device synchronously on startup so first TTS works
        // Use a timeout and fallback to Unknown on any error
        app.current_device = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::task::spawn_blocking(|| {
                inkworm::tts::device::detect_output_kind()
                    .unwrap_or(inkworm::tts::OutputKind::Unknown)
            }),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(inkworm::tts::OutputKind::Unknown);

        if needs_wizard {
            app.open_wizard(WizardOrigin::FirstRun);
        }
        // Speak the current drill on startup (no-op if no course loaded).
        app.speak_current_drill();
        run_loop(&mut guard, &mut app, task_rx).await
    })?;

    tracing::info!("inkworm shutting down");
    Ok(())
}
