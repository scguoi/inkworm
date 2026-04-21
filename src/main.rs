use std::path::PathBuf;
use std::sync::Arc;

use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::load_course;
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::ui::config_wizard::WizardOrigin;
use inkworm::ui::event::run_loop;
use inkworm::ui::terminal::{install_panic_hook, TerminalGuard};

fn main() -> anyhow::Result<()> {
    install_panic_hook();

    let cli_config: Option<PathBuf> = std::env::args()
        .nth(1)
        .filter(|a| a == "--config")
        .and_then(|_| std::env::args().nth(2))
        .map(PathBuf::from);

    let paths = DataPaths::resolve(cli_config.as_deref())?;
    paths.ensure_dirs()?;

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

    let course = progress
        .active_course_id
        .as_deref()
        .and_then(|id| load_course(&paths.courses_dir, id).ok());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let mut guard = TerminalGuard::new()?;
        let (task_tx, task_rx) = tokio::sync::mpsc::channel(32);
        let mut app = App::new(
            course,
            progress,
            paths,
            Arc::new(SystemClock),
            config,
            task_tx,
        );
        if needs_wizard {
            app.open_wizard(WizardOrigin::FirstRun);
        }
        run_loop(&mut guard, &mut app, task_rx).await
    })?;

    Ok(())
}
