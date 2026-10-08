//! Synchronization utilities and notification primitives.

use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll, Waker};

pub use parking_lot::{Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Error indicating failure to receive a notification in [`VersionReceiver::try_recv`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    /// No newer version has been published since the last receive.
    Empty,
    /// The notifier has closed or all senders were dropped.
    Closed,
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "no new version available"),
            Self::Closed => write!(f, "version notifier closed"),
        }
    }
}

impl std::error::Error for TryRecvError {}

/// Shared internal state for a [`VersionedNotifier`] and its subscribers.
struct NotifierState {
    version: AtomicUsize,
    closed: AtomicBool,
    sender_count: AtomicUsize,
    subscriber_count: AtomicUsize,
    next_sub_id: AtomicU64,
    wakers: Mutex<HashMap<u64, Waker>>,
}

/// Monotonic version counter with single-slot coalescing change notifications.
///
/// Unlike unbounded queues, [`VersionedNotifier`] holds a single version counter,
/// ensuring that rapid mutations never cause memory growth or queue bloat.
///
/// Senders can publish new versions without blocking, and subscribers can either
/// synchronously inspect the latest version via [`VersionReceiver::try_recv`] or
/// asynchronously await changes using [`futures::Stream`].
pub struct VersionedNotifier {
    state: OnceLock<Arc<NotifierState>>,
}

impl fmt::Debug for VersionedNotifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VersionedNotifier")
            .field("version", &self.version())
            .field("listeners", &self.listener_count())
            .field("is_closed", &self.is_closed())
            .finish()
    }
}

impl Default for VersionedNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for VersionedNotifier {
    fn clone(&self) -> Self {
        let state = self.state().clone();
        state.sender_count.fetch_add(1, Ordering::Relaxed);
        let cell = OnceLock::new();
        let _ = cell.set(state);
        Self { state: cell }
    }
}

impl Drop for VersionedNotifier {
    fn drop(&mut self) {
        if let Some(state) = self.state.get() {
            if state.sender_count.fetch_sub(1, Ordering::AcqRel) == 1 {
                state.closed.store(true, Ordering::Release);
                let wakers: Vec<Waker> = {
                    let mut lock = state.wakers.lock();
                    lock.drain().map(|(_, w)| w).collect()
                };
                for waker in wakers {
                    waker.wake();
                }
            }
        }
    }
}

impl VersionedNotifier {
    /// Creates a new [`VersionedNotifier`] with initial version `0`.
    ///
    /// This function is `const` so that it can be stored directly in `static` variables.
    pub const fn new() -> Self {
        Self {
            state: OnceLock::new(),
        }
    }

    #[inline]
    fn state(&self) -> &Arc<NotifierState> {
        self.state.get_or_init(|| {
            Arc::new(NotifierState {
                version: AtomicUsize::new(0),
                closed: AtomicBool::new(false),
                sender_count: AtomicUsize::new(1),
                subscriber_count: AtomicUsize::new(0),
                next_sub_id: AtomicU64::new(1),
                wakers: Mutex::new(HashMap::new()),
            })
        })
    }

    /// Returns the current monotonic version counter.
    #[inline]
    pub fn version(&self) -> usize {
        match self.state.get() {
            Some(state) => state.version.load(Ordering::Acquire),
            None => 0,
        }
    }

    /// Checks whether this notifier has been closed.
    #[inline]
    pub fn is_closed(&self) -> bool {
        match self.state.get() {
            Some(state) => state.closed.load(Ordering::Acquire),
            None => false,
        }
    }

    /// Explicitly closes the notifier, causing all current and future receivers
    /// to observe EOF or [`TryRecvError::Closed`].
    pub fn close(&self) {
        let state = self.state();
        state.closed.store(true, Ordering::Release);
        let wakers: Vec<Waker> = {
            let mut lock = state.wakers.lock();
            lock.drain().map(|(_, w)| w).collect()
        };
        for waker in wakers {
            waker.wake();
        }
    }

    /// Atomically increments the monotonic version and notifies all active subscribers.
    ///
    /// Returns the newly incremented version.
    pub fn notify(&self) -> usize {
        let state = self.state();
        let new_version = state.version.fetch_add(1, Ordering::Release) + 1;

        // Drain wakers while holding the lock, but invoke wake() OUTSIDE the lock
        // to prevent lock contention and avoid deadlocks on re-entrant callbacks.
        let wakers: Vec<Waker> = {
            let mut lock = state.wakers.lock();
            lock.drain().map(|(_, w)| w).collect()
        };

        for waker in wakers {
            waker.wake();
        }

        new_version
    }

    /// Subscribes to version updates, returning a [`VersionReceiver`].
    pub fn subscribe(&self) -> VersionReceiver {
        let state = self.state();
        state.subscriber_count.fetch_add(1, Ordering::Relaxed);
        let id = state.next_sub_id.fetch_add(1, Ordering::Relaxed);
        let current_version = state.version.load(Ordering::Acquire);

        VersionReceiver {
            state: state.clone(),
            id,
            last_seen_version: current_version,
        }
    }

    /// Returns the count of active listeners (primarily for diagnostics and testing).
    #[inline]
    pub fn listener_count(&self) -> usize {
        match self.state.get() {
            Some(state) => state.subscriber_count.load(Ordering::Relaxed),
            None => 0,
        }
    }
}

/// Receiver handle for version updates published by [`VersionedNotifier`].
pub struct VersionReceiver {
    state: Arc<NotifierState>,
    id: u64,
    last_seen_version: usize,
}

impl fmt::Debug for VersionReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VersionReceiver")
            .field("id", &self.id)
            .field("last_seen_version", &self.last_seen_version)
            .field(
                "current_version",
                &self.state.version.load(Ordering::Acquire),
            )
            .field("is_closed", &self.is_closed())
            .finish()
    }
}

impl Clone for VersionReceiver {
    fn clone(&self) -> Self {
        self.state.subscriber_count.fetch_add(1, Ordering::Relaxed);
        let id = self.state.next_sub_id.fetch_add(1, Ordering::Relaxed);
        Self {
            state: self.state.clone(),
            id,
            last_seen_version: self.last_seen_version,
        }
    }
}

impl Drop for VersionReceiver {
    fn drop(&mut self) {
        self.state.subscriber_count.fetch_sub(1, Ordering::Relaxed);
        self.state.wakers.lock().remove(&self.id);
    }
}

impl VersionReceiver {
    /// Tries to receive the latest version without blocking.
    ///
    /// Returns `Ok(version)` if a newer version has been published since the last receive.
    /// Returns `Err(TryRecvError::Empty)` if no new version is available.
    /// Returns `Err(TryRecvError::Closed)` if the notifier has closed or all senders dropped.
    #[inline]
    pub fn try_recv(&mut self) -> Result<usize, TryRecvError> {
        let latest = self.state.version.load(Ordering::Acquire);
        if latest != self.last_seen_version {
            self.last_seen_version = latest;
            Ok(latest)
        } else if self.state.closed.load(Ordering::Acquire) {
            Err(TryRecvError::Closed)
        } else {
            Err(TryRecvError::Empty)
        }
    }

    /// Returns the last version seen by this receiver.
    #[inline]
    pub fn last_seen_version(&self) -> usize {
        self.last_seen_version
    }

    /// Checks whether a newer version has been published without updating `last_seen_version`.
    #[inline]
    pub fn has_changed(&self) -> bool {
        self.state.version.load(Ordering::Acquire) != self.last_seen_version
    }

    /// Checks whether the notifier has closed.
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.state.closed.load(Ordering::Acquire)
    }
}

impl futures::Stream for VersionReceiver {
    type Item = usize;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let latest = self.state.version.load(Ordering::Acquire);
        if latest != self.last_seen_version {
            self.last_seen_version = latest;
            return Poll::Ready(Some(latest));
        }

        if self.state.closed.load(Ordering::Acquire) {
            return Poll::Ready(None);
        }

        // Register or update waker
        {
            let mut wakers = self.state.wakers.lock();
            let should_insert = match wakers.get(&self.id) {
                Some(existing) => !existing.will_wake(cx.waker()),
                None => true,
            };
            if should_insert {
                wakers.insert(self.id, cx.waker().clone());
            }
        }

        // Re-check to avoid lost wakeups between initial load and lock registration
        let latest = self.state.version.load(Ordering::Acquire);
        if latest != self.last_seen_version {
            self.last_seen_version = latest;
            self.state.wakers.lock().remove(&self.id);
            return Poll::Ready(Some(latest));
        }

        if self.state.closed.load(Ordering::Acquire) {
            self.state.wakers.lock().remove(&self.id);
            return Poll::Ready(None);
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versioned_notifier_initial_state() {
        let notifier = VersionedNotifier::new();
        assert_eq!(notifier.version(), 0);
        assert_eq!(notifier.listener_count(), 0);
        assert!(!notifier.is_closed());
    }

    #[test]
    fn test_versioned_notifier_version_increment_and_notification() {
        let notifier = VersionedNotifier::new();
        let mut sub1 = notifier.subscribe();
        let mut sub2 = notifier.subscribe();

        notifier.notify();
        assert_eq!(notifier.version(), 1);

        assert_eq!(sub1.try_recv(), Ok(1));
        assert_eq!(sub2.try_recv(), Ok(1));

        notifier.notify();
        assert_eq!(notifier.version(), 2);

        assert_eq!(sub1.try_recv(), Ok(2));
        assert_eq!(sub2.try_recv(), Ok(2));
    }

    #[test]
    fn test_versioned_notifier_cleans_up_dropped_subscribers() {
        let notifier = VersionedNotifier::new();
        let sub1 = notifier.subscribe();
        let mut sub2 = notifier.subscribe();

        assert_eq!(notifier.listener_count(), 2);
        drop(sub1);
        assert_eq!(notifier.listener_count(), 1);

        notifier.notify();
        assert_eq!(notifier.version(), 1);
        assert_eq!(sub2.try_recv(), Ok(1));
        assert_eq!(notifier.listener_count(), 1);
    }

    #[test]
    fn test_versioned_notifier_coalescing() {
        let notifier = VersionedNotifier::new();
        let mut sub = notifier.subscribe();

        notifier.notify();
        notifier.notify();
        notifier.notify();

        assert_eq!(notifier.version(), 3);
        assert_eq!(sub.try_recv(), Ok(3));
        assert_eq!(sub.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn test_version_receiver_stream() {
        use futures::StreamExt;
        let notifier = VersionedNotifier::new();
        let mut sub = notifier.subscribe();

        let notifier_clone = notifier.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            notifier_clone.notify();
        });

        let next = futures::executor::block_on(sub.next());
        assert_eq!(next, Some(1));
        handle.join().unwrap();
    }

    #[test]
    fn test_notifier_drop_closes_stream_and_receiver() {
        use futures::StreamExt;
        let notifier = VersionedNotifier::new();
        let mut sub = notifier.subscribe();

        assert_eq!(sub.try_recv(), Err(TryRecvError::Empty));
        drop(notifier);

        assert_eq!(sub.try_recv(), Err(TryRecvError::Closed));
        let next = futures::executor::block_on(sub.next());
        assert_eq!(next, None);
    }

    #[test]
    fn test_notifier_explicit_close() {
        use futures::StreamExt;
        let notifier = VersionedNotifier::new();
        let mut sub = notifier.subscribe();

        notifier.notify();
        assert_eq!(sub.try_recv(), Ok(1));

        notifier.close();
        assert!(notifier.is_closed());
        assert!(sub.is_closed());
        assert_eq!(sub.try_recv(), Err(TryRecvError::Closed));

        let next = futures::executor::block_on(sub.next());
        assert_eq!(next, None);
    }

    #[test]
    fn test_receiver_clone() {
        let notifier = VersionedNotifier::new();
        let mut sub1 = notifier.subscribe();
        notifier.notify();

        let mut sub2 = sub1.clone();
        assert_eq!(sub1.try_recv(), Ok(1));
        assert_eq!(sub2.try_recv(), Ok(1));
    }

    #[test]
    fn test_parking_lot_reexports() {
        let mutex = Mutex::new(123);
        *mutex.lock() += 1;
        assert_eq!(*mutex.lock(), 124);

        let rwlock = RwLock::new(456);
        assert_eq!(*rwlock.read(), 456);
        *rwlock.write() = 789;
        assert_eq!(*rwlock.read(), 789);
    }
}
