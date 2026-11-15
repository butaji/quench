//! The marker thread of a concurrent cycle: its stack, its handoff with the mutator through one
//! lock and a condition variable, and the chunks it marks between them.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Values the marker marks between looks at its stop flag and its deferred handoff.
const MARKER_CHUNK: usize = 4096;
/// Deferred values the marker keeps before publishing them to the mutator mid-chunk.
const MARKER_DEFERRED_PUBLISH: usize = 1024;
/// The chunks of work the marker holds before it shares some: a backlog of two.
const BACKLOG_CHUNKS: usize = 2;
/// The divisor of the work a marker shares: its half.
const SHARE_DIVISOR: usize = 2;

/// What the marker and the mutator exchange, under one lock.
#[derive(Default)]
struct Handoff {
    /// Values for the marker: the barrier's copies, weak reads, and what the mutator's share
    /// reached.
    inbox: Vec<Value>,
    /// Values the marker left to the mutator, unmarked.
    deferred: Vec<Value>,
    /// Whether the marker is waiting with nothing to mark.
    idle: bool,
}

struct MarkerShared {
    view: ConcurrentMarkView,
    handoff: Mutex<Handoff>,
    wake: Condvar,
    stop: AtomicBool,
    /// Whether the mutator assists (`MarkerThread::assist`): the marker then shares half of a
    /// large stack through the inbox.
    assist: AtomicBool,
    /// Marks set by the marker and assists, and the marker's busy time.
    marked: AtomicU64,
    busy_nanos: AtomicU64,
    /// The longest the marker's stack grew, in values.
    peak_work: AtomicU64,
}

/// The marker thread of a running cycle. Dropping it stops and joins the thread.
pub(in crate::heap) struct MarkerThread {
    shared: Arc<MarkerShared>,
    thread: Option<std::thread::JoinHandle<Vec<Value>>>,
}

impl MarkerThread {
    /// Starts the marker on `work`.
    pub(in crate::heap) fn start(view: ConcurrentMarkView, work: Vec<Value>) -> Self {
        let shared = Arc::new(MarkerShared {
            view,
            handoff: Mutex::new(Handoff { inbox: work, ..Handoff::default() }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            assist: AtomicBool::new(false),
            marked: AtomicU64::new(0),
            busy_nanos: AtomicU64::new(0),
            peak_work: AtomicU64::new(0),
        });
        let marker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("quench-gc-marker".into())
            .spawn(move || marker_loop(&marker))
            .expect("concurrent GC marker thread");
        Self { shared, thread: Some(thread) }
    }

    /// What the marker reads.
    pub(in crate::heap) fn view(&self) -> &ConcurrentMarkView {
        &self.shared.view
    }

    /// Hands `values` to the marker.
    pub(in crate::heap) fn send(&self, values: &mut Vec<Value>) {
        if values.is_empty() {
            return;
        }
        let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
        handoff.inbox.append(values);
        handoff.idle = false;
        drop(handoff);
        self.shared.wake.notify_one();
    }

    /// Takes what the marker deferred into `deferred`; whether the marker is idle with nothing
    /// sent and nothing deferred, so its marking is done unless the caller adds more.
    pub(in crate::heap) fn take_deferred(&self, deferred: &mut Vec<Value>) -> bool {
        let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
        let quiet = handoff.idle && handoff.inbox.is_empty() && handoff.deferred.is_empty();
        deferred.append(&mut handoff.deferred);
        quiet
    }

    /// Marks up to `budget` values of the marker's inbox on the mutator, through the same
    /// view and atomic marks, when allocation outran the cycle: what they reach goes back to
    /// the inbox, and what the view cannot read to `deferred`. From now on the marker shares
    /// half of a large stack through the inbox, so an assist finds work.
    pub(in crate::heap) fn assist(&self, budget: usize, deferred: &mut Vec<Value>) {
        self.shared.assist.store(true, Ordering::Relaxed);
        let mut work = {
            let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
            let keep = handoff.inbox.len().saturating_sub(budget);
            handoff.inbox.split_off(keep)
        };
        let mut marked = 0;
        for _ in 0..budget {
            let Some(value) = work.pop() else { break };
            self.shared.view.mark_value(value, &mut work, deferred, &mut marked);
        }
        self.shared.marked.fetch_add(marked as u64, Ordering::Relaxed);
        self.send(&mut work);
    }

    /// Marks set and the marker's busy time so far, in nanoseconds.
    pub(in crate::heap) fn stats(&self) -> (u64, u64) {
        (self.shared.marked.load(Ordering::Relaxed), self.shared.busy_nanos.load(Ordering::Relaxed))
    }

    /// The longest the marker's stack grew, in values.
    pub(in crate::heap) fn peak_work(&self) -> u64 {
        self.shared.peak_work.load(Ordering::Relaxed)
    }

    /// Stops and joins the marker: every value it held or was sent, and what it deferred.
    pub(in crate::heap) fn stop(mut self) -> Vec<Value> {
        self.stop_and_join()
    }

    pub(in crate::heap) fn stop_and_join(&mut self) -> Vec<Value> {
        let Some(thread) = self.thread.take() else { return Vec::new() };
        self.shared.stop.store(true, Ordering::Release);
        {
            // Under the lock, so a marker about to wait sees the flag first.
            let _handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
            self.shared.wake.notify_one();
        }
        let mut left = thread.join().expect("concurrent GC marker panicked");
        let mut handoff = self.shared.handoff.lock().expect("marker handoff poisoned");
        left.append(&mut handoff.inbox);
        left.append(&mut handoff.deferred);
        left
    }
}

impl Drop for MarkerThread {
    pub(in crate::heap) fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// The marker thread: marks what it is sent until stopped, publishing what it defers, and
/// returns the work it held when stopped.
fn marker_loop(shared: &MarkerShared) -> Vec<Value> {
    let mut work = Vec::new();
    let mut deferred = Vec::new();
    let mut marked = 0;
    let mut busy_since = std::time::Instant::now();
    loop {
        if shared.stop.load(Ordering::Acquire) {
            publish(shared, &mut marked, busy_since);
            work.append(&mut deferred);
            return work;
        }
        if work.is_empty() {
            publish(shared, &mut marked, busy_since);
            let mut handoff = shared.handoff.lock().expect("marker handoff poisoned");
            handoff.deferred.append(&mut deferred);
            let Some(_handoff) = await_inbox(shared, handoff, &mut work) else { return work };
            busy_since = std::time::Instant::now();
        }
        for _ in 0..MARKER_CHUNK {
            let Some(value) = work.pop() else { break };
            shared.view.mark_value(value, &mut work, &mut deferred, &mut marked);
        }
        shared.peak_work.fetch_max(work.capacity() as u64, Ordering::Relaxed);
        let share = shared.assist.load(Ordering::Relaxed) && work.len() > BACKLOG_CHUNKS * MARKER_CHUNK;
        if deferred.len() >= MARKER_DEFERRED_PUBLISH || share {
            hand_off(shared, &mut work, &mut deferred, share);
        }
    }
}

/// Adds the marker's busy time and the marks it made to the shared totals, and clears them.
fn publish(shared: &MarkerShared, marked: &mut usize, busy_since: std::time::Instant) {
    shared.marked.fetch_add(*marked as u64, Ordering::Relaxed);
    shared.busy_nanos.fetch_add(busy_since.elapsed().as_nanos() as u64, Ordering::Relaxed);
    *marked = 0;
}

/// Waits, idle, until the inbox holds work or the marker stops, then takes that work into
/// `work`; `None` when the marker stops first. A large inbox (the initial mark's snapshot)
/// is taken a chunk at a time, so the marker's stack grows from a chunk, not from the whole
/// inbox.
fn await_inbox<'a>(
    shared: &'a MarkerShared,
    mut handoff: std::sync::MutexGuard<'a, Handoff>,
    work: &mut Vec<Value>,
) -> Option<std::sync::MutexGuard<'a, Handoff>> {
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return None;
        }
        if !handoff.inbox.is_empty() {
            let len = handoff.inbox.len();
            if len > BACKLOG_CHUNKS * MARKER_CHUNK {
                work.extend(handoff.inbox.drain(len - MARKER_CHUNK..));
            } else {
                std::mem::swap(work, &mut handoff.inbox);
            }
            handoff.idle = false;
            return Some(handoff);
        }
        handoff.idle = true;
        handoff = shared.wake.wait(handoff).expect("marker handoff poisoned");
    }
}

/// Gives the mutator the deferred values, and when `share` a half of the work, if the inbox is
/// free to take it.
fn hand_off(shared: &MarkerShared, work: &mut Vec<Value>, deferred: &mut Vec<Value>, share: bool) {
    let mut handoff = shared.handoff.lock().expect("marker handoff poisoned");
    handoff.deferred.append(deferred);
    if share && handoff.inbox.is_empty() {
        handoff.inbox = work.split_off(work.len() / SHARE_DIVISOR);
    }
}
