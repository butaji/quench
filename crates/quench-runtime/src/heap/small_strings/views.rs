//! The views of written small strings: a read of one answers a string value sharing its
//! record's text, kept until the next collection.

use super::*;
use std::cell::UnsafeCell;

/// Views per chunk of `SmallViews`.
const VIEW_CHUNK: usize = 1024;
/// A view and the local id of the small string it shows.
type ViewChunk = [(u32, Option<JsString>); VIEW_CHUNK];
/// Views made since the last collection past which the next safepoint collects, which drops
/// them: about 2 MiB of views at most.
pub(super) const VIEW_COLLECTION_LIMIT: u32 = 1 << 16;
/// Chunks a collection keeps for the views after it.
const KEPT_VIEW_CHUNKS: usize = 1;

/// The strings that answer reads of written small strings since the last collection, each a
/// view sharing its record's text storage. A reference to one lives no longer than the shared
/// borrow of the heap it came from, and every collection takes the heap mutably first, so a
/// collection drops them all (`clear`); a record names its latest by slot, checked by owner.
/// Chunks never move while views are handed out.
#[derive(Default)]
pub(super) struct SmallViewChunks {
    chunks: Vec<NonNull<ViewChunk>>,
    len: u32,
}

impl SmallViewChunks {
    fn entry(&self, slot: u32) -> *mut (u32, Option<JsString>) {
        let chunk = self.chunks[slot as usize / VIEW_CHUNK];
        // SAFETY: the chunk is live and the index is within it.
        unsafe { chunk.as_ptr().cast::<(u32, Option<JsString>)>().add(slot as usize % VIEW_CHUNK) }
    }

    /// The view at `slot` when it shows the small string `owner`.
    pub(super) fn get(&self, slot: u32, owner: u32) -> Option<&JsString> {
        if slot >= self.len {
            return None;
        }
        // SAFETY: slots below `len` hold views, none of which is being written.
        let (shown, view) = unsafe { &*self.entry(slot) };
        (*shown == owner).then(|| view.as_ref().expect("a slot below the length holds a view"))
    }

    fn push(&mut self, owner: u32, view: JsString) -> u32 {
        let slot = self.len;
        if slot as usize == self.chunks.len() * VIEW_CHUNK {
            let chunk: Box<ViewChunk> = Box::new([const { (0, None) }; VIEW_CHUNK]);
            self.chunks.push(NonNull::from(Box::leak(chunk)));
        }
        // SAFETY: the slot at `len` holds no view, and nothing refers to it.
        unsafe { *self.entry(slot) = (owner, Some(view)) };
        self.len += 1;
        slot
    }

    /// Drops every view; keeps `KEPT_VIEW_CHUNKS` chunks for the next.
    fn clear(&mut self) {
        for slot in 0..self.len {
            // SAFETY: the caller holds the heap mutably, so no view is referred to.
            unsafe { (*self.entry(slot)).1 = None };
        }
        self.len = 0;
        for chunk in self.chunks.drain(KEPT_VIEW_CHUNKS.min(self.chunks.len())..) {
            // SAFETY: each chunk came from `Box::leak`, holds no view now, and is freed once.
            drop(unsafe { Box::from_raw(chunk.as_ptr()) });
        }
    }

    /// Bytes its chunks take.
    pub(super) fn allocated_bytes(&self) -> usize {
        self.chunks.len() * size_of::<ViewChunk>()
    }
}

impl Drop for SmallViewChunks {
    fn drop(&mut self) {
        self.clear();
        for chunk in self.chunks.drain(..) {
            // SAFETY: as in `clear`.
            drop(unsafe { Box::from_raw(chunk.as_ptr()) });
        }
    }
}

/// `SmallViewChunks` behind a shared heap: a read makes a view through `&Heap`.
#[derive(Default)]
pub(super) struct SmallViews(UnsafeCell<SmallViewChunks>);

impl SmallViews {
    /// Drops every view: at a collection, which holds the heap mutably.
    pub(super) fn clear(&mut self) {
        self.0.get_mut().clear();
    }

    pub(super) fn get(&self) -> &SmallViewChunks {
        // SAFETY: the views change only through `push_shared`, which forms no lasting reference.
        unsafe { &*self.0.get() }
    }

    /// Keeps `view` of the small string `owner`, through a shared heap.
    pub(super) fn push_shared(&self, owner: u32, view: JsString) -> u32 {
        // SAFETY: the heap is single-threaded and no `&mut SmallViewChunks` is live while it is
        // shared; views already handed out sit in chunks this does not move or write.
        unsafe { (*self.0.get()).push(owner, view) }
    }
}
