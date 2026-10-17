//! A boxed slice behind a one-word pointer: the length lives at the front of the allocation,
//! so a cell that holds one stays a word smaller than with `Box<[T]>`.
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::ptr::NonNull;

/// The header every empty slice shares: an empty slice owns no allocation, so making one and
/// dropping it cost nothing.
static EMPTY_HEADER: usize = 0;

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

    fn elements(&self) -> *mut T {
        if self.len() == 0 {
            return NonNull::dangling().as_ptr();
        }
        let (_, offset) = Self::layout(self.len());
        // SAFETY: the allocation spans the header and `len` elements from `offset`.
        unsafe { self.header.as_ptr().cast::<u8>().add(offset).cast::<T>() }
    }

    pub(crate) fn len(&self) -> usize {
        // SAFETY: the header was initialized at allocation and never changes.
        unsafe { *self.header.as_ptr() }
    }

    /// The slice of `values`, written straight into its one allocation.
    pub(crate) fn from_exact_iter(values: impl ExactSizeIterator<Item = T>) -> Self {
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
        let mut written = 0;
        for value in values {
            assert!(written < len, "exact-size iterator yielded more than its length");
            // SAFETY: slot `written` lies within the `len` elements the layout reserves.
            unsafe { raw.add(offset).cast::<T>().add(written).write(value) };
            written += 1;
        }
        // A short iterator leaks the allocation rather than expose uninitialized elements.
        assert_eq!(written, len, "exact-size iterator yielded less than its length");
        // SAFETY: the header slot starts the allocation.
        unsafe { header.as_ptr().write(len) };
        Self {
            header,
            marker: std::marker::PhantomData,
        }
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
        Self::from_exact_iter(self.iter().cloned())
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
        let empty: ThinBoxSlice<u64> = Vec::new().into_boxed_slice().into();
        assert!(empty.is_empty());
        assert_eq!(size_of::<ThinBoxSlice<u64>>(), size_of::<usize>());
    }
}
