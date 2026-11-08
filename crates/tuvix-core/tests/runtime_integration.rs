//! M2 integration gate: the runtime end to end.
//!
//! The M1 gates pin rendering soundness; these pin the runtime contracts:
//! the input stack applied through a real backend, the async event stream
//! awaited from a park-based executor (no async runtime dependency), the
//! runner's restore-on-drop under an injected panic, and the panic hook
//! chain reaching a platform-style restorer.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use tuvix_core::backend::{Backend, ClearRegion, TestBackend};
use tuvix_core::buffer::DiffRun;
use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use tuvix_core::geometry::{Position, Size};
use tuvix_core::runtime::input::{InputMode, InputStack, KittyEnhancementFlags};
use tuvix_core::runtime::panic::{self, RestoreSpec};
use tuvix_core::runtime::queue::event_channel;
use tuvix_core::runtime::runner::AppRunner;
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

/// Serializes the tests that touch the process-global panic hook/registry.
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

fn global_guard() -> std::sync::MutexGuard<'static, ()> {
    match GLOBAL_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// A backend whose operation log survives the runner being moved and dropped
/// inside `catch_unwind`.
#[derive(Debug, Clone, Default)]
struct SharedLogBackend {
    log: Arc<Mutex<Vec<String>>>,
}

impl SharedLogBackend {
    fn new() -> Self {
        Self::default()
    }
}

impl Backend for SharedLogBackend {
    type Error = io::Error;

    fn draw<'a>(
        &mut self,
        _content: impl IntoIterator<Item = DiffRun<'a>>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(Size::new(10, 2))
    }

    fn get_cursor_position(&self) -> Result<Position, Self::Error> {
        Ok(Position::new(0, 0))
    }

    fn set_cursor_position(&mut self, _position: Position) -> Result<(), Self::Error> {
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.log.lock().unwrap().push("show_cursor".to_string());
        Ok(())
    }

    fn clear(&mut self, _region: ClearRegion) -> Result<(), Self::Error> {
        Ok(())
    }

    fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
        self.log.lock().unwrap().push(sequence.to_string());
        Ok(())
    }
}

/// A park-based executor (std-only): proves the stream works with no async
/// runtime in the loop.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct ParkWake(thread::Thread);

    impl Wake for ParkWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ParkWake(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park_timeout(Duration::from_millis(10)),
        }
    }
}

#[test]
fn runner_applies_input_modes_through_the_backend() {
    let terminal = Terminal::new(TestBackend::new(Size::new(20, 3))).unwrap();
    let mut runner = AppRunner::new(terminal);

    runner.push_input_mode(InputMode::FocusChange).unwrap();
    runner
        .push_input_mode(InputMode::KittyKeyboard(
            KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        ))
        .unwrap();
    runner.push_input_mode(InputMode::BracketedPaste).unwrap();

    assert_eq!(
        runner.terminal().backend().control_log(),
        ["\x1b[?1004h", "\x1b[>1u", "\x1b[?2004h"].map(String::from)
    );

    // LIFO pop: bracketed paste first.
    let popped = runner.pop_input_mode().unwrap();
    assert_eq!(popped, Some(InputMode::BracketedPaste));
    assert_eq!(
        runner.terminal().backend().control_log().last(),
        Some(&"\x1b[?2004l".to_string())
    );

    runner.restore().unwrap();
    assert_eq!(
        runner.terminal().backend().control_log(),
        [
            "\x1b[?1004h",
            "\x1b[>1u",
            "\x1b[?2004h",
            "\x1b[?2004l",
            "\x1b[<1u",
            "\x1b[?1004l",
        ]
        .map(String::from)
    );
    assert_eq!(runner.input_stack().modes().len(), 0);
    assert!(!runner.terminal().backend().is_cursor_hidden());
}

#[test]
fn frames_render_through_the_runner_while_modes_are_active() {
    let terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    let mut runner = AppRunner::new(terminal);
    runner.push_input_mode(InputMode::FocusChange).unwrap();

    runner
        .draw(|frame| {
            frame.buffer.set_string(0, 0, "m2 gate", Style::default());
        })
        .unwrap();
    assert_eq!(
        runner.terminal().backend().buffer().to_string(),
        "m2 gate   \n          "
    );
    runner.restore().unwrap();
}

#[test]
fn event_stream_awaits_across_threads_with_no_executor() {
    let (sender, mut stream) = event_channel();

    let producer = thread::spawn(move || {
        for code in [
            KeyCode::Char('a'),
            KeyCode::Char('b'),
            KeyCode::Char('c'),
            KeyCode::Char('d'),
            KeyCode::Char('e'),
        ] {
            assert!(sender.send(Event::Key(KeyEvent::new(code, KeyModifiers::empty()))));
        }
    });

    let received: Vec<KeyCode> = block_on(async {
        let mut codes = Vec::new();
        while codes.len() < 5 {
            if let Some(Event::Key(key)) = block_on(stream.next_event()) {
                codes.push(key.code);
            }
        }
        codes
    });

    producer.join().unwrap();
    assert_eq!(
        received,
        vec![
            KeyCode::Char('a'),
            KeyCode::Char('b'),
            KeyCode::Char('c'),
            KeyCode::Char('d'),
            KeyCode::Char('e'),
        ]
    );
}

#[test]
fn stream_ends_after_the_last_sender_is_dropped() {
    let (sender, mut stream) = event_channel();
    assert!(sender.send(Event::Resize(80, 24)));
    drop(sender);

    // Drain: the queued event first, then the end of the stream.
    assert_eq!(stream.try_next(), Some(Event::Resize(80, 24)));
    assert_eq!(block_on(stream.next_event()), None);
    assert!(stream.is_finished());
}

#[test]
fn panicking_through_the_runner_restores_the_terminal_via_drop() {
    let backend = SharedLogBackend::new();
    let log = backend.log.clone();
    let terminal = Terminal::new(backend).unwrap();
    let mut runner = AppRunner::new(terminal);
    runner.push_input_mode(InputMode::MouseCapture).unwrap();
    runner.push_input_mode(InputMode::FocusChange).unwrap();
    log.lock().unwrap().clear();

    // Panicking with the runner on the stack must unwind through
    // AppRunner::drop, which restores the terminal.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut runner = runner;
        runner.draw(|_| {}).unwrap();
        panic!("tuvix-unwind-gate");
    }));

    assert!(result.is_err(), "the injected panic must propagate");
    let entries = log.lock().unwrap().clone();
    // LIFO: focus was pushed last, so it unwinds first; then the mouse
    // disable set; then the cursor is shown.
    assert_eq!(
        entries,
        vec![
            "\x1b[?1004l".to_string(),
            "\x1b[?1006l".to_string(),
            "\x1b[?1015l".to_string(),
            "\x1b[?1003l".to_string(),
            "\x1b[?1002l".to_string(),
            "\x1b[?1000l".to_string(),
            "show_cursor".to_string(),
        ]
    );
}

#[test]
fn panic_hook_chain_reaches_the_platform_restorer_with_the_spec() {
    let _guard = global_guard();
    let original = std::panic::take_hook();
    let restored = Arc::new(AtomicBool::new(false));
    let seen_spec: Arc<Mutex<Vec<InputMode>>> = Arc::new(Mutex::new(Vec::new()));

    // Simulate what try_init does: runner + install_panic_hook.
    let terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    let mut runner = AppRunner::new(terminal);
    runner.push_input_mode(InputMode::BracketedPaste).unwrap();
    runner.push_input_mode(InputMode::FocusChange).unwrap();

    {
        let restored = Arc::clone(&restored);
        let seen_spec = Arc::clone(&seen_spec);
        runner.install_panic_hook(move |spec: &RestoreSpec| {
            restored.store(true, Ordering::SeqCst);
            *seen_spec.lock().unwrap() = spec.modes().to_vec();
        });
    }

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        panic!("tuvix-integration-panic");
    }));

    assert!(
        restored.load(Ordering::SeqCst),
        "restorer must run on panic"
    );
    assert_eq!(
        *seen_spec.lock().unwrap(),
        vec![InputMode::BracketedPaste, InputMode::FocusChange]
    );

    // Runner drop unregisters the spec; the hook chain is restored.
    drop(runner);
    assert_eq!(panic::current_restore_spec().modes().len(), 0);
    std::panic::set_hook(original);
}

#[test]
fn restore_spec_matches_the_input_stack_restore() {
    let mut stack = InputStack::new();
    stack.push(InputMode::MouseCapture);
    stack.push(InputMode::KittyKeyboard(
        KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KittyEnhancementFlags::REPORT_EVENT_TYPES,
    ));
    stack.push(InputMode::BracketedPaste);

    let spec = RestoreSpec::from_modes(stack.modes().iter().copied());
    let mut restored: Vec<String> = stack
        .restore()
        .disable_sequences()
        .iter()
        .map(|sequence| sequence.clone().into_owned())
        .collect();
    restored.push("\x1b[?25h".to_string());

    assert_eq!(spec.disable_sequences(), restored);
}

#[test]
fn key_event_kinds_survive_the_full_queue_path() {
    let (sender, mut stream) = event_channel();
    let release = KeyEvent {
        code: KeyCode::Char('x'),
        modifiers: KeyModifiers::empty(),
        kind: KeyEventKind::Release,
        state: KeyEventState::KEYPAD,
    };
    assert!(sender.send(Event::Key(release)));
    assert_eq!(stream.try_next(), Some(Event::Key(release)));
}
