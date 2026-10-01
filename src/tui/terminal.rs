use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event as TermEvent, EventStream, KeyEventKind,
};
use crossterm::execute;
use futures_util::StreamExt;
use tokio::sync::mpsc;

use super::view;
use crate::controller::Controller;

const FRAME: Duration = Duration::from_millis(100);

/// Run the UI until the user quits. Redraws only after something changed,
/// and ticks only while something animates.
pub async fn run() -> io::Result<()> {
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let mut controller = Controller::new(events_tx);
    controller.start();

    let mut terminal = ratatui::try_init()?;
    execute!(io::stdout(), EnableMouseCapture)?;
    let result = event_loop(&mut terminal, &mut controller, &mut events).await;
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    controller.shutdown().await;
    result
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    controller: &mut Controller,
    events: &mut mpsc::UnboundedReceiver<crate::controller::Event>,
) -> io::Result<()> {
    let mut input = EventStream::new();
    let mut tick = tokio::time::interval(FRAME);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut dirty = true;
    while !controller.app.quit {
        if dirty {
            terminal.draw(|frame| view::draw(frame, &mut controller.app))?;
        }
        let animating = controller.app.animating() || view::marquee_active(&controller.app);
        dirty = tokio::select! {
            event = input.next() => match event {
                Some(Ok(TermEvent::Key(key))) if key.kind != KeyEventKind::Release => {
                    let cmds = controller.app.handle_key(key);
                    controller.run(cmds);
                    true
                }
                Some(Ok(TermEvent::Mouse(mouse))) => {
                    let cmds = controller.app.handle_mouse(mouse);
                    controller.run(cmds);
                    true
                }
                Some(Ok(TermEvent::Resize(..))) => true,
                Some(Ok(_)) => false,
                Some(Err(error)) => return Err(error),
                None => return Ok(()),
            },
            Some(event) = events.recv() => {
                controller.handle(event);
                while let Ok(event) = events.try_recv() {
                    controller.handle(event);
                }
                true
            }
            _ = tick.tick(), if animating => controller.app.tick(Instant::now()),
        };
    }
    Ok(())
}
