//! A boxed slice behind a one-word pointer: the length lives at the front of the allocation,
//! so a cell that holds one stays a word smaller than with `Box<[T]>`.
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::ptr::NonNull;

/// The header every empty slice shares: an empty slice owns no allocation, so making one and
/// dropping it cost nothing.
static EMPTY_HEADER: usize = 0;
/// The header's top bit: a mark its owner may set on a non-empty slice (`ThinBoxSlice::mark`)
/// to tell two layouts of its elements apart; the length is the rest of the header.
const MARK_BIT: usize = 1 << (usize::BITS - 1);

#[repr(transparent)]
pub(crate) struct ThinBoxSlice<T> {
    /// Points at the header; the elements follow at `elements_offset`.
    header: NonNull<usize>,
    marker: std::marker::PhantomData<T>,
}

impl<T> ThinBoxSlice<T> {
    fn layout(len: usize) -> (Layout, usize) {
        let (layout, offset) = Layout::new::<usize>()
            .extend(Layout::array::<T>(len).expect("slice layout overflow"))
            .expect("slice layout overflow");
        (layout.pad_to_align(), offset)
    }

    fn empty() -> Self {
        Self {
            header: NonNull::from(&EMPTY_HEADER),
            marker: std::marker::PhantomData,
        }
    }

    /// Where the elements start: after the header, at `T`'s alignment. `layout` derives the
    /// same offset for every length.
    const ELEMENTS_OFFSET: usize = size_of::<usize>().next_multiple_of(align_of::<T>());

    fn elements(&self) -> *mut T {
        if self.len() == 0 {
            return NonNull::dangling().as_ptr();
        }
        debug_assert_eq!(Self::layout(self.len()).1, Self::ELEMENTS_OFFSET);
        // SAFETY: the allocation spans the header and `len` elements from `ELEMENTS_OFFSET`.
        unsafe { self.header.as_ptr().cast::<u8>().add(Self::ELEMENTS_OFFSET).cast::<T>() }
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.header() & !MARK_BIT
    }

    #[inline(always)]
    fn header(&self) -> usize {
        // SAFETY: the header was initialized at allocation; only `mark` changes it, and only
        // in an allocation this slice owns.
        unsafe { *self.header.as_ptr() }
    }

    /// Whether `mark` was set.
    #[inline(always)]
    pub(crate) fn marked(&self) -> bool {
        self.header() & MARK_BIT != 0
    }

    /// Sets the mark of a non-empty slice, whose header it owns.
    pub(crate) fn mark(&mut self) {
        assert!(self.len() != 0, "an empty slice shares the static header");
        // SAFETY: a non-empty slice owns its allocation, header included.
        unsafe { *self.header.as_ptr() |= MARK_BIT };
    }

    /// Words a slice laid out in place (`lay_out_in_place`) takes before its elements: the
    /// slice itself, then its header.
    pub(crate) const IN_PLACE_WORDS: usize = 2;

    /// Lays out at `words` a slice of the `len` elements that follow its `IN_PLACE_WORDS`,
    /// and returns it: a view that owns nothing, which its caller must never drop, valid
    /// while the words stay put and hold their layout.
    ///
    /// # Safety
    /// `words` is valid for writes of `IN_PLACE_WORDS` words and the `len` initialized
    /// elements follow them.
    pub(crate) unsafe fn lay_out_in_place(words: *mut usize, len: usize) -> *const Self {
        const { assert!(Self::ELEMENTS_OFFSET == size_of::<usize>()) };
        debug_assert_eq!(len & MARK_BIT, 0);
        // SAFETY: as the caller guarantees; the header word precedes the elements by
        // `ELEMENTS_OFFSET`, as an allocated slice's does.
        unsafe {
            let header = words.add(1);
            header.write(len);
            words.write(header as usize);
        }
        words.cast::<Self>()
    }

    /// A slice of `len` elements made in place by `element(index)`, or `None` when one is
    /// missing, which drops those made before it.
    pub(crate) fn try_from_fn(len: usize, mut element: impl FnMut(usize) -> Option<T>) -> Option<Self> {
        /// Owns the allocation and its initialized prefix until every element exists.
        struct Partial<T> {
            raw: *mut u8,
            layout: Layout,
            elements: *mut T,
            initialized: usize,
        }
        impl<T> Drop for Partial<T> {
            fn drop(&mut self) {
                // SAFETY: the prefix is initialized and the allocation came from `alloc` with
                // this layout.
                unsafe {
                    std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(self.elements, self.initialized));
                    dealloc(self.raw, self.layout);
                }
            }
        }
        if len == 0 {
            return Some(Self::empty());
        }
        let (layout, offset) = Self::layout(len);
        // SAFETY: the layout is non-zero sized, as it always holds the header.
        let raw = unsafe { alloc(layout) };
        let Some(header) = NonNull::new(raw.cast::<usize>()) else {
            handle_alloc_error(layout)
        };
        let mut partial = Partial {
            raw,
            layout,
            // SAFETY: the allocation spans the header and `len` elements from `offset`.
            elements: unsafe { raw.add(offset).cast::<T>() },
            initialized: 0,
        };
        while partial.initialized < len {
            let value = element(partial.initialized)?;
            // SAFETY: the element lies within the allocation and is not yet initialized.
            unsafe { partial.elements.add(partial.initialized).write(value) };
            partial.initialized += 1;
        }
        std::mem::forget(partial);
        // SAFETY: the header slot starts the allocation.
        unsafe { header.as_ptr().write(len) };
        Some(Self {
            header,
            marker: std::marker::PhantomData,
        })
    }

    /// The slice of `values`, written straight into its one allocation.
    pub(crate) fn from_exact_iter(mut values: impl ExactSizeIterator<Item = T>) -> Self {
        let slice = Self::try_from_fn(values.len(), |_| values.next())
            .expect("exact-size iterator yielded less than its length");
        assert!(values.next().is_none(), "exact-size iterator yielded more than its length");
        slice
    }
}

impl<T> Default for ThinBoxSlice<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T> From<Vec<T>> for ThinBoxSlice<T> {
    fn from(values: Vec<T>) -> Self {
        values.into_boxed_slice().into()
    }
}

impl<'a, T> IntoIterator for &'a ThinBoxSlice<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T> FromIterator<T> for ThinBoxSlice<T> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        values.into_iter().collect::<Vec<_>>().into()
    }
}

impl<T: PartialEq> PartialEq for ThinBoxSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T> From<Box<[T]>> for ThinBoxSlice<T> {
    fn from(values: Box<[T]>) -> Self {
        let len = values.len();
        if len == 0 {
            return Self::empty();
        }
        let (layout, offset) = Self::layout(len);
        // SAFETY: the layout is non-zero sized, as it always holds the header.
        let raw = unsafe { alloc(layout) };
        let Some(header) = NonNull::new(raw.cast::<usize>()) else {
            handle_alloc_error(layout)
        };
        // SAFETY: the new allocation fits the header and `len` elements; the elements move
        // out of `values`, whose own allocation is then freed without dropping them.
        unsafe {
            header.as_ptr().write(len);
            let elements = raw.add(offset).cast::<T>();
            let source = Box::into_raw(values);
            std::ptr::copy_nonoverlapping(source.cast::<T>(), elements, len);
            drop(Box::from_raw(source as *mut [std::mem::ManuallyDrop<T>]));
        }
        Self {
            header,
            marker: std::marker::PhantomData,
        }
    }
}

impl<T> std::ops::Deref for ThinBoxSlice<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        // SAFETY: `len` initialized elements start at `elements`, owned by `self`.
        unsafe { std::slice::from_raw_parts(self.elements(), self.len()) }
    }
}

impl<T> std::ops::DerefMut for ThinBoxSlice<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: as in `deref`, with `&mut self` making the borrow unique.
        unsafe { std::slice::from_raw_parts_mut(self.elements(), self.len()) }
    }
}

impl<T> Drop for ThinBoxSlice<T> {
    fn drop(&mut self) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let (layout, _) = Self::layout(len);
        // SAFETY: the elements are initialized and dropped once; the allocation came from
        // `alloc` with this layout.
        unsafe {
            std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(self.elements(), len));
            dealloc(self.header.as_ptr().cast::<u8>(), layout);
        }
    }
}

impl<T: Clone> Clone for ThinBoxSlice<T> {
    fn clone(&self) -> Self {
        let mut clone = Self::from_exact_iter(self.iter().cloned());
        if self.marked() {
            clone.mark();
        }
        clone
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for ThinBoxSlice<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::ThinBoxSlice;

    #[test]
    fn empty_thin_slices_share_the_static_header() {
        let empty: ThinBoxSlice<std::rc::Rc<i32>> = Vec::new().into_boxed_slice().into();
        let other = ThinBoxSlice::<std::rc::Rc<i32>>::from_exact_iter(std::iter::empty());
        assert!(empty.is_empty() && other.is_empty());
        assert_eq!(empty.header, other.header);
        let copy = empty.clone();
        assert_eq!(copy.len(), 0);
        let built = ThinBoxSlice::from_exact_iter([1, 2, 3].into_iter());
        assert_eq!(&*built, &[1, 2, 3]);
    }

    #[test]
    fn thin_slices_round_trip_and_drop_their_elements() {
        let shared = std::rc::Rc::new(7);
        let slice: ThinBoxSlice<std::rc::Rc<i32>> = vec![shared.clone(), shared.clone()]
            .into_boxed_slice()
            .into();
        assert_eq!(slice.len(), 2);
        assert_eq!(std::rc::Rc::strong_count(&shared), 3);
        let copy = slice.clone();
        assert_eq!(std::rc::Rc::strong_count(&shared), 5);
        drop(slice);
        drop(copy);
        assert_eq!(std::rc::Rc::strong_count(&shared), 1);
        let made = ThinBoxSlice::try_from_fn(3, |_| Some(shared.clone())).unwrap();
        assert_eq!(std::rc::Rc::strong_count(&shared), 4);
        drop(made);
        assert!(ThinBoxSlice::try_from_fn(3, |index| (index < 2).then(|| shared.clone())).is_none());
        assert_eq!(std::rc::Rc::strong_count(&shared), 1);
        let empty: ThinBoxSlice<u64> = Vec::new().into_boxed_slice().into();
        assert!(empty.is_empty());
        assert_eq!(size_of::<ThinBoxSlice<u64>>(), size_of::<usize>());
    }
}
