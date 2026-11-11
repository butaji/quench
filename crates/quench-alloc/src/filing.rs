//! How a heap files its pages: the one transition function between page states and the
//! intrusive lists a state names.

use super::*;

/// Where a page is filed in its owner heap, which fixes the list that links it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum PageState {
    /// In no list and not current: a full page, or a just initialized one before it is filed.
    Full,
    /// The page its class allocates from (`Heap::current`), in no list.
    Current,
    /// Linked in its class's `partial` list, with blocks in use and free blocks to take.
    Partial,
    /// Linked in the `empty` pool, with no block in use.
    Empty,
}

impl Heap {
    /// Files a page that just gained free blocks: the empty pool when it has none in use,
    /// else its class's partial list, unless it is the class's current page.
    #[inline(always)]
    pub(super) unsafe fn settle(&mut self, page: *mut Page) {
        // SAFETY: an initialized page of this heap.
        let to = unsafe {
            match ((*page).state, (*page).used) {
                (PageState::Current, _) | (PageState::Partial, 1..) => return,
                (_, 0) => PageState::Empty,
                (_, _) => PageState::Partial,
            }
        };
        // SAFETY: as above.
        unsafe { self.file_page(page, to) };
    }

    /// The one place a page changes state: unlinks it from the list its state names, files it
    /// under `to`, and keeps `current` naming exactly the pages in `Current`. A page made
    /// current displaces the class's previous one, which is full: the allocator moves on only
    /// when it can take no block. Pages in the pool are reached only through `new_page`.
    #[inline(never)]
    pub(super) unsafe fn file_page(&mut self, page: *mut Page, to: PageState) {
        // SAFETY: `page` is an initialized page of this heap, filed as its state says.
        unsafe {
            let class = usize::from((*page).class);
            let from = (*page).state;
            // A pool page is initialized (`Full`) before it is filed again.
            debug_assert!(from != to && from != PageState::Empty);
            debug_assert_eq!(from == PageState::Current, self.current[class] == page);
            debug_assert!(to != PageState::Empty || (*page).used == 0);
            if from == PageState::Partial {
                self.unlink_partial(page);
            }
            match to {
                PageState::Full => {}
                PageState::Current => {
                    let displaced = std::mem::replace(&mut self.current[class], page);
                    if !displaced.is_null() {
                        (*displaced).state = PageState::Full;
                    }
                }
                PageState::Partial => self.link_partial(page, class),
                PageState::Empty => {
                    (*page).next = self.empty;
                    self.empty = page;
                }
            }
            (*page).state = to;
        }
    }

    unsafe fn link_partial(&mut self, page: *mut Page, class: usize) {
        // SAFETY: `page` is an initialized page of `class` not in any list.
        unsafe {
            (*page).prev = ptr::null_mut();
            (*page).next = self.partial[class];
            if !self.partial[class].is_null() {
                (*self.partial[class]).prev = page;
            }
        }
        self.partial[class] = page;
    }

    unsafe fn unlink_partial(&mut self, page: *mut Page) {
        // SAFETY: `page` is linked in its class's partial list.
        unsafe {
            let class = usize::from((*page).class);
            if (*page).prev.is_null() {
                self.partial[class] = (*page).next;
            } else {
                (*(*page).prev).next = (*page).next;
            }
            if !(*page).next.is_null() {
                (*(*page).next).prev = (*page).prev;
            }
            (*page).prev = ptr::null_mut();
            (*page).next = ptr::null_mut();
        }
    }
}
