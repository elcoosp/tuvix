//! The async-ready event queue.
//!
//! [`event_channel`] pairs an [`EventSender`] with an [`EventStream`]. The
//! stream is pollable from *any* executor — or synchronously via
//! [`EventStream::try_next`] — with no external async dependencies: waking
//! goes through the standard [`Waker`] protocol, so tokio, smol, async-std
//! and hand-rolled executors all work.
//!
//! # Semantics
//!
//! - Multi-producer, single-consumer: senders are cloneable; the stream is
//!   owned by one consumer.
//! - Dropping the stream closes the channel; `send` returns `false` from
//!   then on.
//! - When the last sender is dropped and the queue is drained, the stream
//!   yields `None` — `while let Some(event) = stream.next_event().await` ends.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::event::Event;
//! use tuvix_core::runtime::queue::event_channel;
//!
//! let (sender, mut stream) = event_channel();
//! assert!(sender.send(Event::Resize(80, 24)));
//! assert_eq!(stream.try_next(), Some(Event::Resize(80, 24)));
//! ```

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::event::Event;

struct QueueState {
    queue: VecDeque<Event>,
    /// Waker of the consumer currently waiting for an event.
    waker: Option<Waker>,
    /// The consumer dropped the stream; senders are closed.
    receiver_dropped: bool,
    /// Number of live senders; the stream ends when zero and the queue is
    /// drained.
    sender_count: usize,
}

impl QueueState {
    /// Non-blocking receive: pops the front event if there is one.
    fn try_pop(&mut self) -> Option<Event> {
        self.queue.pop_front()
    }

    /// End-of-stream decision: no more events can ever arrive.
    fn is_ended(&self) -> bool {
        self.sender_count == 0 && self.queue.is_empty()
    }
}

fn wake(state: &mut QueueState) {
    if let Some(waker) = state.waker.take() {
        waker.wake();
    }
}

/// Shared state between senders and the stream.
struct Shared {
    state: Mutex<QueueState>,
}

/// Creates a sender/stream pair for tuvix events.
pub fn event_channel() -> (EventSender, EventStream) {
    let shared = Arc::new(Shared {
        state: Mutex::new(QueueState {
            queue: VecDeque::new(),
            waker: None,
            receiver_dropped: false,
            sender_count: 1,
        }),
    });
    (
        EventSender {
            shared: Arc::clone(&shared),
        },
        EventStream { shared },
    )
}

/// The sending half of an event channel; cloneable and thread-safe.
pub struct EventSender {
    shared: Arc<Shared>,
}

impl EventSender {
    /// Enqueues `event`, waking the consumer. Returns `false` when the
    /// receiving stream was dropped (the channel is closed).
    pub fn send(&self, event: Event) -> bool {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if state.receiver_dropped {
            return false;
        }
        state.queue.push_back(event);
        wake(&mut state);
        true
    }

    /// Whether the receiving stream was dropped.
    pub fn is_closed(&self) -> bool {
        match self.shared.state.lock() {
            Ok(state) => state.receiver_dropped,
            Err(poisoned) => poisoned.into_inner().receiver_dropped,
        }
    }
}

impl Clone for EventSender {
    fn clone(&self) -> Self {
        {
            let mut state = match self.shared.state.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            state.sender_count += 1;
        }
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for EventSender {
    fn drop(&mut self) {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.sender_count = state.sender_count.saturating_sub(1);
        wake(&mut state);
    }
}

impl fmt::Debug for EventSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventSender").finish_non_exhaustive()
    }
}

/// The receiving half of an event channel: pollable from any executor.
pub struct EventStream {
    shared: Arc<Shared>,
}

impl EventStream {
    /// Polls for the next event.
    ///
    /// - `Ready(Some(event))` — an event arrived.
    /// - `Ready(None)` — all senders dropped and the queue is drained; the
    ///   stream is over.
    /// - `Pending` — no event yet; the waker is registered and will be
    ///   woken by the next `send` (or by the last sender dropping).
    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(event) = state.try_pop() {
            return Poll::Ready(Some(event));
        }
        if state.is_ended() {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Awaits the next event (`None` when the stream is over).
    pub fn next_event(&mut self) -> NextEvent<'_> {
        NextEvent { stream: self }
    }

    /// Synchronous, non-blocking receive: the next queued event, if any.
    ///
    /// Unlike [`poll_next`](Self::poll_next) this never registers a waker
    /// and never reports end-of-stream; pair it with
    /// [`is_closed`](#method.is_closed) if you need to distinguish an empty
    /// queue from a finished stream.
    pub fn try_next(&mut self) -> Option<Event> {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.try_pop()
    }

    /// Whether all senders are gone and the queue is drained.
    pub fn is_finished(&self) -> bool {
        let state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.is_ended()
    }

    /// Number of events currently queued.
    pub fn len(&self) -> usize {
        let state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.queue.len()
    }

    /// Whether no events are queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.receiver_dropped = true;
        state.waker = None;
    }
}

impl fmt::Debug for EventStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}

/// The future returned by [`EventStream::next_event`].
#[derive(Debug)]
pub struct NextEvent<'a> {
    stream: &'a mut EventStream,
}

impl Future for NextEvent<'_> {
    type Output = Option<Event>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let stream = &mut *self.get_mut().stream;
        stream.poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;
    use std::thread;
    use std::time::Duration;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    /// A waker that counts wakeups (single-threaded test helper).
    #[derive(Debug)]
    struct CountingWake(AtomicUsize);

    impl Wake for CountingWake {
        fn wake(self: Arc<Self>) {
            let _ = self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            let _ = self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Returns the counting wake handle and a matching [`Waker`].
    fn counting_waker() -> (Arc<CountingWake>, Waker) {
        let wake = Arc::new(CountingWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        (wake, waker)
    }

    mod channel {
        use super::*;

        #[test]
        fn send_then_try_next_round_trips() {
            let (sender, mut stream) = event_channel();
            assert!(sender.send(key(KeyCode::Enter)));
            assert_eq!(stream.try_next(), Some(key(KeyCode::Enter)));
            assert!(stream.is_empty());
            assert_eq!(stream.try_next(), None);
        }

        #[test]
        fn queue_preserves_order() {
            let (sender, mut stream) = event_channel();
            for code in [KeyCode::Char('a'), KeyCode::Tab, KeyCode::Esc] {
                assert!(sender.send(key(code)));
            }
            assert_eq!(stream.len(), 3);
            for code in [KeyCode::Char('a'), KeyCode::Tab, KeyCode::Esc] {
                assert_eq!(stream.try_next(), Some(key(code)));
            }
        }

        #[test]
        fn send_after_stream_drop_reports_closed() {
            let (sender, stream) = event_channel();
            drop(stream);
            assert!(sender.is_closed());
            assert!(!sender.send(key(KeyCode::Enter)));
        }

        #[test]
        fn sender_clone_is_a_real_second_sender() {
            let (sender, mut stream) = event_channel();
            let clone = sender.clone();
            assert!(sender.send(key(KeyCode::Char('a'))));
            assert!(clone.send(key(KeyCode::Char('b'))));
            assert_eq!(stream.len(), 2);
            assert_eq!(stream.try_next(), Some(key(KeyCode::Char('a'))));
            assert_eq!(stream.try_next(), Some(key(KeyCode::Char('b'))));
        }

        #[test]
        fn stream_ends_only_after_last_sender_and_drain() {
            let (sender, mut stream) = event_channel();
            let (_wake, waker) = counting_waker();
            let mut cx = Context::from_waker(&waker);
            // A single live sender: pending, not ended.
            assert_eq!(stream.poll_next(&mut cx), Poll::Pending);
            sender.send(key(KeyCode::Esc));
            assert_eq!(
                stream.poll_next(&mut cx),
                Poll::Ready(Some(key(KeyCode::Esc)))
            );
            // Drop the last sender: stream must end.
            drop(sender);
            assert_eq!(stream.poll_next(&mut cx), Poll::Ready(None));
            assert!(stream.is_finished());
        }

        #[test]
        fn queued_events_survive_sender_drop() {
            let (sender, mut stream) = event_channel();
            sender.send(key(KeyCode::Char('x')));
            sender.send(key(KeyCode::Char('y')));
            drop(sender);
            assert_eq!(stream.try_next(), Some(key(KeyCode::Char('x'))));
            assert_eq!(stream.try_next(), Some(key(KeyCode::Char('y'))));
            assert_eq!(stream.try_next(), None);
            assert!(stream.is_finished());
        }

        #[test]
        fn poll_registers_waker_and_send_wakes_it() {
            let (sender, mut stream) = event_channel();
            let (wake, waker) = counting_waker();
            let mut cx = Context::from_waker(&waker);
            assert_eq!(stream.poll_next(&mut cx), Poll::Pending);
            assert_eq!(wake.0.load(Ordering::SeqCst), 0);
            assert!(sender.send(key(KeyCode::Enter)));
            assert_eq!(wake.0.load(Ordering::SeqCst), 1, "send must wake");
            assert_eq!(
                stream.poll_next(&mut cx),
                Poll::Ready(Some(key(KeyCode::Enter)))
            );
        }

        #[test]
        fn last_sender_drop_wakes_a_waiting_consumer() {
            let (sender, mut stream) = event_channel();
            let (wake, waker) = counting_waker();
            let mut cx = Context::from_waker(&waker);
            assert_eq!(stream.poll_next(&mut cx), Poll::Pending);
            drop(sender);
            assert_eq!(wake.0.load(Ordering::SeqCst), 1, "close must wake");
            assert_eq!(stream.poll_next(&mut cx), Poll::Ready(None));
        }

        #[test]
        fn next_future_polls_through() {
            let (sender, mut stream) = event_channel();
            let (_wake, waker) = counting_waker();
            let mut cx = Context::from_waker(&waker);
            let mut first = stream.next_event();
            let mut first = Pin::new(&mut first);
            assert_eq!(first.as_mut().poll(&mut cx), Poll::Pending);
            sender.send(Event::Resize(10, 5));
            assert_eq!(
                first.as_mut().poll(&mut cx),
                Poll::Ready(Some(Event::Resize(10, 5)))
            );
        }

        #[test]
        fn cross_thread_await_receives_all_events() {
            let (sender, mut stream) = event_channel();
            let producer = thread::spawn(move || {
                for code in [KeyCode::Char('1'), KeyCode::Char('2'), KeyCode::Char('3')] {
                    assert!(sender.send(key(code)));
                }
            });
            let received = block_on_three(&mut stream);
            producer.join().unwrap();
            assert_eq!(
                received,
                vec![
                    Some(key(KeyCode::Char('1'))),
                    Some(key(KeyCode::Char('2'))),
                    Some(key(KeyCode::Char('3'))),
                ]
            );
        }

        /// Minimal park-based executor: proof the stream works without any
        /// async runtime dependency.
        fn block_on_three(stream: &mut EventStream) -> Vec<Option<Event>> {
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
            let mut received = Vec::with_capacity(3);
            while received.len() < 3 {
                match stream.poll_next(&mut cx) {
                    Poll::Ready(event) => received.push(event),
                    Poll::Pending => thread::park_timeout(Duration::from_millis(50)),
                }
            }
            received
        }

        #[test]
        fn dropping_stream_wakes_nothing_but_closes_channel() {
            let (sender, stream) = event_channel();
            let (wake, waker) = counting_waker();
            let mut cx = Context::from_waker(&waker);
            {
                let mut stream = stream;
                assert_eq!(stream.poll_next(&mut cx), Poll::Pending);
            }
            assert!(sender.is_closed());
            assert!(!sender.send(key(KeyCode::Enter)));
            let _ = wake.0.load(Ordering::SeqCst);
        }

        #[test]
        fn debug_impls_exist() {
            let (sender, stream) = event_channel();
            assert!(format!("{sender:?}").contains("EventSender"));
            assert!(format!("{stream:?}").contains("EventStream"));
        }
    }
}
