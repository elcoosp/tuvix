//! The background event reader: termion input pumped into an
//! [`EventStream`].
//!
//! [`TermionEvents::spawn`] starts a reader thread ("tuvix-events") that
//! polls a [`EventPoller`] with a 250 ms deadline (so a shutdown request
//! is honored promptly), translates native termion events with the
//! lossless [`to_tuvix_event`] bridge and forwards them into the tuvix
//! event queue. The application awaits the paired [`EventStream`] with any
//! executor — or drains it synchronously.
//!
//! The production poller, [`StdinPoller`], owns a second thread
//! ("tuvix-termion-stdin") that reads stdin bytes into a shared queue.
//! Unlike the reader thread it blocks on the OS read: it cannot be
//! interrupted portably, so it exits at EOF (the same contract termion's
//! own `async_stdin` keeps). The reader thread itself is stop-flagged and
//! joinable; on stdin EOF the poller errors and the stream ends.
//!
//! The polling itself is abstracted behind [`EventPoller`], so the reader
//! loop is fully testable without a terminal (a scripted poller feeds
//! events and the loop's behavior is asserted end to end).

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use termion::event::Event as TermionEvent;
use tuvix_core::runtime::queue::{EventSender, EventStream, event_channel};

use crate::event::to_tuvix_event;

/// A source of native termion events, abstracted for testability.
pub trait EventPoller: Send + 'static {
    /// Waits up to `timeout` for an event to become available.
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;

    /// Reads one native event. Only called right after a `poll` that
    /// returned `Ok(true)`.
    fn read(&mut self) -> io::Result<TermionEvent>;
}

/// The byte queue shared between the stdin pump thread and the poller.
struct ByteQueue {
    bytes: Mutex<QueueState>,
    available: Condvar,
}

struct QueueState {
    pending: VecDeque<u8>,
    eof: bool,
}

impl ByteQueue {
    fn new() -> Self {
        Self {
            bytes: Mutex::new(QueueState {
                pending: VecDeque::new(),
                eof: false,
            }),
            available: Condvar::new(),
        }
    }

    /// Waits up to `timeout` until at least one byte is pending or stdin
    /// reached EOF. Returns `true` when a byte is available.
    fn wait_for_byte(&self, timeout: Duration) -> bool {
        let guard = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (guard, _timeout) = self
            .available
            .wait_timeout_while(guard, timeout, |state| {
                state.pending.is_empty() && !state.eof
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !guard.pending.is_empty()
    }

    /// Pops one byte, waiting until one arrives or stdin reaches EOF.
    fn take_byte(&self) -> Option<u8> {
        let mut guard = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(byte) = guard.pending.pop_front() {
                return Some(byte);
            }
            if guard.eof {
                return None;
            }
            guard = self
                .available
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Returns `true` when at least one byte is pending right now.
    fn has_pending(&self) -> bool {
        !self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .is_empty()
    }

    /// Appends freshly read bytes and wakes the poller.
    fn push(&self, bytes: &[u8]) {
        let mut guard = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.pending.extend(bytes.iter().copied());
        drop(guard);
        self.available.notify_all();
    }

    /// Marks stdin as finished and wakes the poller.
    fn close(&self) {
        let mut guard = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.eof = true;
        drop(guard);
        self.available.notify_all();
    }
}

/// The production poller: stdin bytes read on a dedicated pump thread and
/// parsed with termion's own event parser.
pub struct StdinPoller {
    queue: Arc<ByteQueue>,
    pump: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for StdinPoller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdinPoller")
            .field("pump_alive", &self.pump.is_some())
            .finish_non_exhaustive()
    }
}

impl StdinPoller {
    /// Starts the stdin pump thread. The thread is named, exits at EOF and
    /// pushes every byte it reads into the shared queue.
    pub fn new() -> Self {
        let queue = Arc::new(ByteQueue::new());
        let thread_queue = Arc::clone(&queue);
        // The pump blocks in read(2): it ends when stdin reaches EOF, like
        // termion's own async reader. Named and observable, not stoppable
        // from outside — document the difference to the reader thread.
        let pump = thread::Builder::new()
            .name("tuvix-termion-stdin".to_string())
            .spawn(move || {
                let mut stdin = io::stdin().lock();
                let mut chunk = [0_u8; 64];
                loop {
                    match stdin.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => thread_queue.push(&chunk[..read]),
                    }
                }
                thread_queue.close();
            })
            .ok();
        Self { queue, pump }
    }

    /// Parses one event from the queued bytes, mirroring termion's own
    /// reader protocol:
    ///
    /// - a lone `ESC` byte with nothing pending behind it is the Esc key;
    /// - a sequence that fails to parse becomes `Event::Unsupported` with
    ///   the consumed bytes (the bridge then drops it, per its policy).
    fn parse_from_queue(&self) -> io::Result<TermionEvent> {
        let first = self
            .queue
            .take_byte()
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "stdin ended"))?;
        if first == b'\x1b' && !self.queue.has_pending() {
            return Ok(TermionEvent::Key(termion::event::Key::Esc));
        }
        let mut consumed = vec![first];
        let mut continuation = QueueIter {
            queue: &self.queue,
            consumed: &mut consumed,
        };
        match termion::event::parse_event(first, &mut continuation) {
            Ok(event) => Ok(event),
            Err(_) => Ok(TermionEvent::Unsupported(consumed)),
        }
    }
}

impl Default for StdinPoller {
    fn default() -> Self {
        Self::new()
    }
}

impl EventPoller for StdinPoller {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        if self.queue.wait_for_byte(timeout) {
            return Ok(true);
        }
        // EOF with an empty queue: stdin is gone, the stream ends.
        Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin ended"))
    }

    fn read(&mut self) -> io::Result<TermionEvent> {
        self.parse_from_queue()
    }
}

/// An `Iterator<Item = io::Result<u8>>` over the shared byte queue, as
/// termion's `parse_event` expects for continuation bytes. Every byte
/// pulled is recorded, so failed parses can surface the consumed bytes as
/// `Event::Unsupported`.
struct QueueIter<'a> {
    queue: &'a ByteQueue,
    consumed: &'a mut Vec<u8>,
}

impl Iterator for QueueIter<'_> {
    type Item = io::Result<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.queue.take_byte() {
            Some(byte) => {
                self.consumed.push(byte);
                Some(Ok(byte))
            }
            None => Some(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "stdin ended",
            ))),
        }
    }
}

impl Drop for StdinPoller {
    fn drop(&mut self) {
        // The pump thread is joined when it exits at EOF; detached drop is
        // the documented contract (stdin reads are not interruptible).
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

/// The termion event reader thread plus its channel.
///
/// Drop stops the thread (joined, with a 250 ms worst-case latency) and the
/// sender, which ends the paired stream after it drains.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(unix)] fn demo() -> std::io::Result<()> {
/// use tuvix_backend_termion::TermionEvents;
///
/// let (_reader, mut stream) = TermionEvents::spawn()?;
/// while let Some(event) = stream.try_next() {
///     std::hint::black_box(event);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct TermionEvents {
    sender: EventSender,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl TermionEvents {
    /// Spawns the termion reader thread and returns it paired with the
    /// stream of translated tuvix events.
    pub fn spawn() -> io::Result<(Self, EventStream)> {
        Self::spawn_with(StdinPoller::new())
    }

    /// Like [`spawn`](Self::spawn) but with a scripted poller (tests).
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

impl Drop for TermionEvents {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The reader loop: poll with a deadline, read, translate, forward.
///
/// Exits when the stop flag is set, the stream is closed, all receivers
/// dropped, or the poller errors. termion events the tuvix model cannot
/// represent are dropped silently (see the bridge docs).
fn run_reader(mut poller: impl EventPoller, sender: &EventSender, stop: &AtomicBool) {
    const POLL_DEADLINE: Duration = Duration::from_millis(250);

    loop {
        if stop.load(Ordering::Relaxed) || sender.is_closed() {
            break;
        }
        match poller.poll(POLL_DEADLINE) {
            Ok(true) => match poller.read() {
                Ok(native) => {
                    if let Some(event) = to_tuvix_event(&native)
                        && !sender.send(event)
                    {
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
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use termion::event::Key as TKey;
    use tuvix_core::event::{Event, KeyCode};

    /// A scripted poller: a queue of pre-programmed results.
    enum ScriptedStep {
        /// `poll` returns ready and `read` returns this event.
        Emit(TermionEvent),
        /// `poll` times out.
        Idle,
        /// `poll` errors (terminal went away).
        Broken,
    }

    struct ScriptedPoller {
        script: Mutex<VecDeque<ScriptedStep>>,
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
            match script.front() {
                Some(ScriptedStep::Emit(_)) => Ok(true),
                Some(ScriptedStep::Idle) => Ok(false),
                Some(ScriptedStep::Broken) | None => {
                    Err(io::Error::new(io::ErrorKind::BrokenPipe, "script over"))
                }
            }
        }

        fn read(&mut self) -> io::Result<TermionEvent> {
            let mut script = self.script.lock().unwrap();
            match script.pop_front() {
                Some(ScriptedStep::Emit(event)) => Ok(event),
                _ => Err(io::Error::new(io::ErrorKind::BrokenPipe, "script over")),
            }
        }
    }

    /// Waits (bounded) for the reader thread to deliver through the queue:
    /// try_next is non-blocking, so tests must not race the worker.
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

    #[test]
    fn reader_forwards_translated_events_in_order() {
        let (mut reader, mut stream) = TermionEvents::spawn_with(ScriptedPoller::new([
            ScriptedStep::Emit(TermionEvent::Key(TKey::Char('a'))),
            ScriptedStep::Emit(TermionEvent::Key(TKey::Char('b'))),
            ScriptedStep::Idle,
            ScriptedStep::Idle,
        ]))
        .unwrap();

        wait_until(
            || stream.len() == 2,
            "both events should arrive through the queue",
        );
        let first = stream.try_next();
        let second = stream.try_next();
        assert!(matches!(first, Some(Event::Key(event)) if event.code == KeyCode::Char('a')));
        assert!(matches!(second, Some(Event::Key(event)) if event.code == KeyCode::Char('b')));
        reader.stop();
    }

    #[test]
    fn unrepresentable_native_events_are_dropped() {
        let (mut reader, mut stream) = TermionEvents::spawn_with(ScriptedPoller::new([
            ScriptedStep::Emit(TermionEvent::Unsupported(vec![1, 2, 3])),
            ScriptedStep::Emit(TermionEvent::Key(TKey::Esc)),
            ScriptedStep::Idle,
            ScriptedStep::Idle,
        ]))
        .unwrap();

        wait_until(
            || stream.len() == 1,
            "the representable event should arrive (the unsupported one dropped)",
        );
        let event = stream.try_next();
        assert!(matches!(event, Some(Event::Key(event)) if event.code == KeyCode::Esc));
        reader.stop();
    }

    #[test]
    fn poller_errors_end_the_stream() {
        let (mut reader, mut stream) =
            TermionEvents::spawn_with(ScriptedPoller::new([ScriptedStep::Broken])).unwrap();
        assert_eq!(stream.try_next(), None);
        reader.stop();
    }

    #[test]
    fn reader_stops_promptly_on_the_stop_flag() {
        let (mut reader, mut stream) =
            TermionEvents::spawn_with(ScriptedPoller::new([ScriptedStep::Idle])).unwrap();
        // The stop flag must end the loop even while the poller idles.
        reader.stop();
        assert_eq!(stream.try_next(), None);
    }

    #[test]
    fn byte_queue_signals_availability_and_eof() {
        let queue = ByteQueue::new();
        assert!(!queue.wait_for_byte(Duration::from_millis(10)));
        queue.push(b"x");
        assert!(queue.wait_for_byte(Duration::from_millis(10)));
        assert_eq!(queue.take_byte(), Some(b'x'));
        queue.close();
        assert_eq!(queue.take_byte(), None);
    }

    #[test]
    fn stdin_poller_parses_bytes_from_its_queue() {
        let queue = Arc::new(ByteQueue::new());
        queue.push(b"q");
        let mut poller = StdinPoller {
            queue: Arc::clone(&queue),
            pump: None,
        };
        // A byte is available; it parses into a plain character key.
        assert!(poller.poll(Duration::from_millis(10)).unwrap());
        assert_eq!(poller.read().unwrap(), TermionEvent::Key(TKey::Char('q')));
    }

    #[test]
    fn stdin_poller_parses_escape_sequences_arriving_together() {
        // ESC + x in one chunk: the parser consumes the continuation byte
        // and reports Alt+x.
        let queue = Arc::new(ByteQueue::new());
        queue.push(b"\x1bx");
        let mut poller = StdinPoller {
            queue: Arc::clone(&queue),
            pump: None,
        };
        assert!(poller.poll(Duration::from_millis(10)).unwrap());
        assert_eq!(poller.read().unwrap(), TermionEvent::Key(TKey::Alt('x')));
    }

    #[test]
    fn stdin_poller_reports_a_lone_escape_as_esc() {
        // termion's protocol: an ESC byte with nothing pending behind it is
        // the Esc key, not the start of a sequence.
        let queue = Arc::new(ByteQueue::new());
        queue.push(b"\x1b");
        let mut poller = StdinPoller {
            queue: Arc::clone(&queue),
            pump: None,
        };
        assert!(poller.poll(Duration::from_millis(10)).unwrap());
        assert_eq!(poller.read().unwrap(), TermionEvent::Key(TKey::Esc));
    }

    #[test]
    fn stdin_poller_surfaces_malformed_sequences_as_unsupported() {
        // ESC [ Q is not a sequence termion knows: the reader must report
        // Unsupported with the consumed bytes (the bridge drops it).
        let queue = Arc::new(ByteQueue::new());
        queue.push(b"\x1b[Q");
        let mut poller = StdinPoller {
            queue: Arc::clone(&queue),
            pump: None,
        };
        assert!(poller.poll(Duration::from_millis(10)).unwrap());
        assert_eq!(
            poller.read().unwrap(),
            TermionEvent::Unsupported(vec![b'\x1b', b'[', b'Q'])
        );
    }

    #[test]
    fn stdin_poller_errors_at_eof() {
        let queue = Arc::new(ByteQueue::new());
        queue.close();
        let mut poller = StdinPoller { queue, pump: None };
        // EOF: poll ends the stream with an error, by contract.
        let error = poller.poll(Duration::from_millis(10)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
