//! Batched retirement of the writable view's pages.
//!
//! Each publication wrote its code through the writable view, whose page-table entries the
//! kernel drops with `madvise(MADV_DONTNEED)` so a resident code page is counted once. That
//! call is a TLB shootdown across every CPU running the process, and one per publication
//! interrupted the program's thread. Spans queue here and go together: adjacent pages of a
//! region leave in one call, when `RETIRE_BATCH_BYTES` are waiting or the code generator
//! idles (`flush`). Until then the written pages are counted twice, at most the batch.

use super::views::{page_bytes, Views};
use std::sync::{Mutex, PoisonError};

/// Written bytes that wait before a flush. A publication averages a few KiB, so this is
/// dozens of publications per call; the double-counted pages add at most this to peak RSS.
const RETIRE_BATCH_BYTES: usize = 256 << 10;

struct Pending {
    spans: Vec<(Views, usize, usize)>,
    bytes: usize,
}

// SAFETY: a `Views` addresses mappings that live for the process; the queue only passes them to `madvise`.
unsafe impl Send for Pending {}

static PENDING: Mutex<Pending> = Mutex::new(Pending { spans: Vec::new(), bytes: 0 });

/// Queues `[offset, offset + bytes)` of `views`' writable view for retirement, flushing the
/// queue once a batch has gathered.
pub(super) fn retire(views: Views, offset: usize, bytes: usize) {
    let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
    pending.spans.push((views, offset, bytes));
    pending.bytes += bytes;
    if pending.bytes >= RETIRE_BATCH_BYTES {
        flush_locked(&mut pending);
    }
}

/// Retires every queued span now.
pub(in crate::vm) fn flush() {
    flush_locked(&mut PENDING.lock().unwrap_or_else(PoisonError::into_inner));
}

fn flush_locked(pending: &mut Pending) {
    pending.spans.sort_unstable_by_key(|(views, offset, _)| (views.write as usize, *offset));
    let mut spans = pending.spans.drain(..).peekable();
    while let Some((views, offset, bytes)) = spans.next() {
        let mut end = offset + bytes;
        while let Some((_, next, next_bytes)) = spans.next_if(|(next_views, next, _)| next_views.write == views.write && *next <= end.next_multiple_of(page_bytes())) {
            end = end.max(next + next_bytes);
        }
        views.retire_writer(offset, end - offset);
    }
    pending.bytes = 0;
}
