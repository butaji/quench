//! A boxed slice behind a one-word pointer: the length lives at the front of the allocation,
//! so a cell that holds one stays a word smaller than with `Box<[T]>`.
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::ptr::NonNull;

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

    fn elements(&self) -> *mut T {
        let (_, offset) = Self::layout(self.len());
        // SAFETY: the allocation spans the header and `len` elements from `offset`.
        unsafe { self.header.as_ptr().cast::<u8>().add(offset).cast::<T>() }
    }

    pub(crate) fn len(&self) -> usize {
        // SAFETY: the header was initialized at allocation and never changes.
        unsafe { *self.header.as_ptr() }
    }
}

impl<T> From<Box<[T]>> for ThinBoxSlice<T> {
    fn from(values: Box<[T]>) -> Self {
        let len = values.len();
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
        self.iter().cloned().collect::<Box<[T]>>().into()
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
