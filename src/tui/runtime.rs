use super::*;

use crossterm::event::EventStream;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub trait TerminalControl: Send {
    fn enter(&mut self) -> io::Result<()>;
    fn restore(&mut self) -> io::Result<()>;
}

pub(crate) struct CrosstermTerminalControl;

impl TerminalControl for CrosstermTerminalControl {
    fn enter(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture) {
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(())
    }

    fn restore(&mut self) -> io::Result<()> {
        let execute_result = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let raw_result = disable_raw_mode();
        execute_result.and(raw_result)
    }
}

pub struct TerminalGuard {
    control: Box<dyn TerminalControl>,
    active: bool,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        Self::with_control(Box::new(CrosstermTerminalControl))
    }

    pub fn with_control(mut control: Box<dyn TerminalControl>) -> io::Result<Self> {
        control.enter()?;
        Ok(Self {
            control,
            active: true,
        })
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        let result = self.control.restore();
        if result.is_ok() {
            self.active = false;
        }
        result
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub async fn run_terminal_app_with_handler<F>(
    app: TuiApp,
    root: CancellationToken,
    mut handler: F,
) -> io::Result<()>
where
    F: FnMut(&mut TuiApp, TuiRuntimeEvent) + Send,
{
    let mut guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;
    terminal.clear()?;
    let result = run_terminal_loop(&mut terminal, app, &root, &mut handler).await;
    guard.restore()?;
    result
}

pub(crate) async fn run_terminal_loop<F>(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    mut app: TuiApp,
    root: &CancellationToken,
    handler: &mut F,
) -> io::Result<()>
where
    F: FnMut(&mut TuiApp, TuiRuntimeEvent) + Send,
{
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.draw(|frame| render_app(frame, &app, None))?;
        let runtime_event = tokio::select! {
            _ = root.cancelled() => break,
            signal = tokio::signal::ctrl_c() => {
                signal?;
                Some(TuiRuntimeEvent::Action(KeyAction::Quit))
            }
            _ = tick.tick() => {
                app.advance_tick();
                Some(TuiRuntimeEvent::Tick)
            }
            event = events.next() => match event {
                Some(Ok(CrosstermEvent::Key(key))) => {
                    let Some(tui_key) = map_crossterm_key(key.code, key.modifiers) else {
                        continue;
                    };
                    app.handle_key_event(tui_key)
                }
                Some(Ok(CrosstermEvent::Mouse(mouse))) => {
                    let Some(kind) = map_crossterm_mouse_kind(mouse.kind) else {
                        continue;
                    };
                    let size = terminal.size()?;
                    app.handle_mouse_event(MouseInput {
                        kind,
                        column: mouse.column,
                        row: mouse.row,
                        area: Rect::new(0, 0, size.width, size.height),
                    })
                }
                Some(Ok(_)) => continue,
                Some(Err(error)) => return Err(error),
                None => break,
            },
        };
        if runtime_event == Some(TuiRuntimeEvent::Action(KeyAction::Quit)) {
            break;
        }
        if let Some(runtime_event) = runtime_event {
            handler(&mut app, runtime_event);
        }
    }
    Ok(())
}

pub async fn shutdown_tasks(tasks: &mut JoinSet<()>, root: &CancellationToken, deadline: Duration) {
    root.cancel();
    if tokio::time::timeout(deadline, async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.abort_all();
        tasks.detach_all();
    }
}

pub(crate) fn map_crossterm_mouse_kind(kind: MouseEventKind) -> Option<MouseInputKind> {
    match kind {
        MouseEventKind::Down(MouseButton::Left) => Some(MouseInputKind::LeftClick),
        MouseEventKind::Up(MouseButton::Left) => Some(MouseInputKind::LeftRelease),
        MouseEventKind::Drag(MouseButton::Left) => Some(MouseInputKind::LeftDrag),
        MouseEventKind::ScrollUp => Some(MouseInputKind::ScrollUp),
        MouseEventKind::ScrollDown => Some(MouseInputKind::ScrollDown),
        _ => None,
    }
}

pub(crate) fn map_crossterm_key(code: KeyCode, modifiers: KeyModifiers) -> Option<KeyEvent> {
    match code {
        KeyCode::Char(value) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(KeyEvent::Ctrl(value))
        }
        KeyCode::Char(value) => Some(KeyEvent::Char(value)),
        KeyCode::Enter => Some(KeyEvent::Enter),
        KeyCode::Backspace => Some(KeyEvent::Backspace),
        KeyCode::Tab => Some(KeyEvent::Tab),
        KeyCode::Esc => Some(KeyEvent::Esc),
        KeyCode::Up => Some(KeyEvent::Up),
        KeyCode::Down => Some(KeyEvent::Down),
        KeyCode::Left => Some(KeyEvent::Left),
        KeyCode::Right => Some(KeyEvent::Right),
        KeyCode::PageUp => Some(KeyEvent::PageUp),
        KeyCode::PageDown => Some(KeyEvent::PageDown),
        KeyCode::Home => Some(KeyEvent::Home),
        KeyCode::End => Some(KeyEvent::End),
        _ => None,
    }
}
