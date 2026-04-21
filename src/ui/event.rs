use std::time::Duration;

use crossterm::event::EventStream;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::time;

use crate::app::App;
use crate::ui::task_msg::TaskMsg;
use crate::ui::terminal::TerminalGuard;

pub async fn run_loop(
    guard: &mut TerminalGuard,
    app: &mut App,
    mut task_rx: mpsc::Receiver<TaskMsg>,
) -> std::io::Result<()> {
    let mut crossterm_stream = EventStream::new();
    let mut tick = time::interval(Duration::from_millis(16));
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        guard.terminal.draw(|f| app.render(f))?;
        tokio::select! {
            Some(Ok(evt)) = crossterm_stream.next() => app.on_input(evt),
            _ = tick.tick() => app.on_tick(),
            Some(msg) = task_rx.recv() => app.on_task_msg(msg),
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}
