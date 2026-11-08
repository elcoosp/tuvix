//! The background event reader: crossterm input pumped into an
//! [`EventStream`].
//!
//! [`CrosstermEvents::spawn`] starts a reader thread that polls crossterm
//! (250 ms deadline so a shutdown request is honored promptly), translates
//! native events with the lossless [`to_tuvix_event`] bridge and forwards
//! them into the tuvix event queue. The application awaits the paired
//! [`EventStream`] with any executor — or drains it synchronously.
//!
//! The crossterm polling itself is abstracted behind [`EventPoller`], so the
//! reader loop is fully testable without a terminal (a scripted poller feeds
//! events and the loop's behavior is asserted end to end).

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossterm::event::{self as crossterm_event, Event as CrosstermEvent};
use tuvix_core::runtime::queue::{EventSender, EventStream, event_channel};

use crate::event::to_tuvix_event;

/// A source of native crossterm events, abstracted for testability.
///
/// The production implementation delegates to crossterm's global
/// [`poll`](crossterm::event::poll)/[`read`](crossterm::event::read); tests
/// script their own.
pub trait EventPoller: Send + 'static {
    /// Waits up to `timeout` for an event to become available.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` when the event source fails
    /// (for example a closed terminal descriptor).
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;

    /// Reads one native event. Only called right after a `poll` that
    /// returned `Ok(true)`.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` when the event cannot be read.
    fn read(&mut self) -> io::Result<CrosstermEvent>;
}

/// The production poller: crossterm's global event source.
#[derive(Debug, Default, Clone, Copy)]
pub struct CrosstermPoller;

impl EventPoller for CrosstermPoller {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        crossterm_event::poll(timeout)
    }

    fn read(&mut self) -> io::Result<CrosstermEvent> {
        crossterm_event::read()
    }
}

/// The crossterm event reader thread plus its channel.
///
/// Drop stops the thread (joined, with a 250 ms worst-case latency) and the
/// sender, which ends the paired stream after it drains.
///
/// # Examples
///
/// ```no_run
/// use tuvix_backend_crossterm::CrosstermEvents;
/// use tuvix_core::runtime::queue::EventStream;
///
/// # fn main() -> std::io::Result<()> {
/// let (_reader, mut stream) = CrosstermEvents::spawn()?;
/// // Any executor can await the stream; this drains it synchronously:
/// while let Some(event) = stream.try_next() {
///     std::hint::black_box(event);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct CrosstermEvents {
    sender: EventSender,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl CrosstermEvents {
    /// Spawns the crossterm reader thread and returns it paired with the
    /// stream of translated tuvix events.
    ///
    /// # Errors
    /// Returns the `io::Error` from spawning the reader thread.
    pub fn spawn() -> io::Result<(Self, EventStream)> {
        Self::spawn_with(CrosstermPoller)
    }

    /// Like [`spawn`](Self::spawn) but with a scripted poller (tests).
    ///
    /// # Errors
    /// Returns the `io::Error` from spawning the reader thread.
    pub fn spawn_with(poller: impl EventPoller) -> io::Result<(Self, EventStream)> {
        let (sender, stream) = event_channel();
        let stop = Arc::new(AtomicBool::new(false));

        let thread_stop = Arc::clone(&stop);
        let thread_sender = sender.clone();
        let worker = thread::Builder::new()
            .name("tuvix-events".to_string())
            .spawn(move || run_reader(poller, &thread_sender, &thread_stop))?;

        let reader = Self {
            sender,
            stop,
            worker: Some(worker),
        };
        Ok((reader, stream))
    }

    /// A handle for injecting synthetic events into the stream (tests,
    /// app-internal signals).
    #[must_use]
    pub fn sender(&self) -> &EventSender {
        &self.sender
    }

    /// Stops the reader thread, joining it. The paired stream ends once the
    /// remaining events are drained.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for CrosstermEvents {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The reader loop: poll with a deadline, read, translate, forward.
///
/// Exits when the stop flag is set, the stream is closed, all receivers
/// dropped, or the poller errors.
fn run_reader(mut poller: impl EventPoller, sender: &EventSender, stop: &AtomicBool) {
    const POLL_DEADLINE: Duration = Duration::from_millis(250);

    loop {
        if stop.load(Ordering::Relaxed) || sender.is_closed() {
            break;
        }
        match poller.poll(POLL_DEADLINE) {
            Ok(true) => match poller.read() {
                Ok(native) => {
                    if !sender.send(to_tuvix_event(native)) {
                        break;
                    }
                }
                Err(_) => break,
            },
            Ok(false) => {}
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        Event as CEvent, KeyCode as CKeyCode, KeyEvent as CKeyEvent, KeyEventKind as CKeyEventKind,
        KeyEventState as CKeyEventState, KeyModifiers as CKeyModifiers,
        MouseButton as CMouseButton, MouseEvent as CMouseEvent, MouseEventKind as CMouseEventKind,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use tuvix_core::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton,
        MouseEvent, MouseEventKind,
    };

    /// A scripted poller: a queue of pre-programmed results.
    /// `Err(Ok)` entries simulate poll=ready/read=event pairs; timing is
    /// irrelevant in tests.
    struct ScriptedPoller {
        script: Mutex<VecDeque<ScriptedStep>>,
    }

    enum ScriptedStep {
        /// `poll` returns ready and `read` returns this event.
        Emit(CEvent),
        /// `poll` times out.
        Idle,
        /// `poll` errors (terminal went away).
        Broken,
    }

    impl ScriptedPoller {
        fn new(steps: impl IntoIterator<Item = ScriptedStep>) -> Self {
            Self {
                script: Mutex::new(steps.into_iter().collect()),
            }
        }
    }

    impl EventPoller for ScriptedPoller {
        fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
            let script = self.script.lock().unwrap();
            let next = script.front();
            Ok(matches!(next, Some(ScriptedStep::Emit(_))))
        }

        fn read(&mut self) -> io::Result<CEvent> {
            let mut script = self.script.lock().unwrap();
            match script.pop_front() {
                Some(ScriptedStep::Emit(event)) => Ok(event),
                Some(ScriptedStep::Idle) => Err(io::Error::other("read while idle")),
                Some(ScriptedStep::Broken) | None => Err(io::Error::other("terminal closed")),
            }
        }
    }

    fn native_key(code: CKeyCode) -> CEvent {
        CEvent::Key(CKeyEvent::new(code, CKeyModifiers::empty()))
    }

    fn wait_until(predicate: impl Fn() -> bool, description: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("condition not met within 2s: {description}");
    }

    mod reader {
        use super::*;

        #[test]
        fn events_are_translated_and_forwarded_in_order() {
            let (mut reader, mut stream) = CrosstermEvents::spawn_with(ScriptedPoller::new([
                ScriptedStep::Emit(native_key(CKeyCode::Enter)),
                ScriptedStep::Emit(native_key(CKeyCode::Char('q'))),
                ScriptedStep::Idle,
                ScriptedStep::Idle,
            ]))
            .unwrap();

            wait_until(
                || stream.len() == 2,
                "both events should arrive through the queue",
            );
            assert_eq!(
                stream.try_next(),
                Some(Event::Key(KeyEvent::new(
                    KeyCode::Enter,
                    KeyModifiers::empty()
                )))
            );
            assert_eq!(
                stream.try_next(),
                Some(Event::Key(KeyEvent::new(
                    KeyCode::Char('q'),
                    KeyModifiers::empty()
                )))
            );
            reader.stop();
        }

        #[test]
        fn mouse_events_map_losslessly_through_the_reader() {
            let native = CEvent::Mouse(CMouseEvent {
                kind: CMouseEventKind::Down(CMouseButton::Left),
                column: 7,
                row: 9,
                modifiers: CKeyModifiers::NONE,
            });
            let (mut reader, mut stream) =
                CrosstermEvents::spawn_with(ScriptedPoller::new([ScriptedStep::Emit(native)]))
                    .unwrap();
            wait_until(|| stream.len() == 1, "mouse event should arrive");
            assert_eq!(
                stream.try_next(),
                Some(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 7,
                    row: 9,
                    modifiers: KeyModifiers::empty(),
                }))
            );
            reader.stop();
        }

        #[test]
        fn synthetic_events_flow_through_the_sender_handle() {
            let (reader, mut stream) =
                CrosstermEvents::spawn_with(ScriptedPoller::new([])).unwrap();
            assert!(reader.sender().send(Event::Resize(100, 50)));
            wait_until(|| stream.len() == 1, "synthetic event should arrive");
            assert_eq!(stream.try_next(), Some(Event::Resize(100, 50)));
            drop(reader);
        }

        #[test]
        fn poller_errors_end_the_stream() {
            let (reader, mut stream) =
                CrosstermEvents::spawn_with(ScriptedPoller::new([ScriptedStep::Broken])).unwrap();
            drop(reader); // stop + join before observing the stream end
            wait_until(|| stream.is_finished(), "stream should end after error");
            assert_eq!(stream.try_next(), None);
        }

        #[test]
        fn dropping_the_stream_stops_the_reader() {
            struct CountingPoller(Arc<AtomicUsize>);
            impl EventPoller for CountingPoller {
                fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
                    let _ = self.0.fetch_add(1, Ordering::Relaxed);
                    Ok(false)
                }

                fn read(&mut self) -> io::Result<CEvent> {
                    Err(io::Error::other("never called"))
                }
            }

            let poller_uses = Arc::new(AtomicUsize::new(0));
            let (reader, stream) =
                CrosstermEvents::spawn_with(CountingPoller(Arc::clone(&poller_uses))).unwrap();

            // Wait until the loop is measurably spinning.
            wait_until(
                || poller_uses.load(Ordering::Relaxed) > 0,
                "reader loop should start polling",
            );

            // After the stream is dropped, the loop must exit within one
            // iteration (it checks the closed stream before each poll).
            drop(stream);
            let before = poller_uses.load(Ordering::Relaxed);
            thread::sleep(Duration::from_millis(400));
            let after = poller_uses.load(Ordering::Relaxed);
            assert!(
                after <= before + 2,
                "reader kept looping {before} -> {after} after stream close"
            );
            drop(reader);
        }

        #[test]
        fn stop_stops_even_when_events_keep_coming() {
            let (mut reader, _stream) = CrosstermEvents::spawn_with(ScriptedPoller::new([
                ScriptedStep::Emit(native_key(CKeyCode::Char('a'))),
                ScriptedStep::Emit(native_key(CKeyCode::Char('b'))),
                ScriptedStep::Emit(native_key(CKeyCode::Char('c'))),
                ScriptedStep::Emit(native_key(CKeyCode::Char('d'))),
                ScriptedStep::Emit(native_key(CKeyCode::Char('e'))),
                ScriptedStep::Emit(native_key(CKeyCode::Char('f'))),
            ]))
            .unwrap();
            reader.stop();
            // stop() joins the worker; nothing to assert beyond it returning.
        }

        #[test]
        fn key_event_state_is_preserved_through_the_reader() {
            let native = CEvent::Key(CKeyEvent {
                code: CKeyCode::Char('x'),
                modifiers: CKeyModifiers::CONTROL,
                kind: CKeyEventKind::Release,
                state: CKeyEventState::KEYPAD,
            });
            let (mut reader, mut stream) =
                CrosstermEvents::spawn_with(ScriptedPoller::new([ScriptedStep::Emit(native)]))
                    .unwrap();
            wait_until(|| stream.len() == 1, "key event should arrive");
            let received = stream.try_next();
            let Some(Event::Key(key)) = received else {
                panic!("expected a key event, got {received:?}");
            };
            assert_eq!(key.code, KeyCode::Char('x'));
            assert_eq!(key.modifiers, KeyModifiers::CONTROL);
            assert_eq!(key.kind, KeyEventKind::Release);
            assert_eq!(key.state, KeyEventState::KEYPAD);
            reader.stop();
        }
    }
}
