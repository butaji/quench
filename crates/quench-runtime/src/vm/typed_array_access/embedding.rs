//! The embedding API over typed arrays and their buffers: the host reads, writes and creates
//! their bytes.

use super::*;

impl<H: Host> Vm<H> {
    pub(crate) fn embedding_array_buffer_aligned_offset(
        &mut self,
        root: RootId,
        alignment: usize,
    ) -> Result<usize, JsError> {
        let program = self.embedding_program()?;
        if !alignment.is_power_of_two() {
            return Err(
                self.type_error(&program, "alignment must be a positive power of two".into())
            );
        }
        let value = self
            .heap
            .root_value(root)
            .ok_or_else(|| self.type_error(&program, "ArrayBuffer root expired".into()))?;
        let Some(Cell::ArrayBuffer {
            bytes,
            detached,
            shared: false,
            resizable,
            ..
        }) = self.heap.get_mut(value)
        else {
            return Err(self.type_error(&program, "Expected an ArrayBuffer".into()));
        };
        if *detached || bytes.is_empty() {
            return Ok(0);
        }
        let byte_length = bytes.len();
        let before = bytes.capacity();
        let address = if *resizable {
            bytes.as_ptr_bytes() as usize
        } else {
            bytes.pin_address()
        };
        let after = bytes.capacity();
        self.heap.adjust_external_bytes(before, after);
        let offset = address.wrapping_neg() & (alignment - 1);
        if offset > byte_length {
            return Err(self.range_error(&program, "Aligned offset exceeds backing buffer".into()));
        }
        Ok(offset)
    }

    /// Pin and describe the bytes an ArrayBuffer or view names (see
    /// [`crate::NativeContext::buffer_storage_rooted`]); `None` for other values.
    pub(crate) fn embedding_buffer_storage(
        &mut self,
        root: RootId,
    ) -> Result<Option<(crate::BufferStorage, Value)>, JsError> {
        use crate::BuiltinBrand as B;
        let program = self.embedding_program()?;
        let value = self
            .heap
            .root_value(root)
            .ok_or_else(|| self.type_error(&program, "buffer root expired".into()))?;
        let (kind, buffer, byte_offset, byte_length, length) = match self.heap.get(value) {
            Some(Cell::ArrayBuffer { shared, .. }) => {
                let kind = if *shared {
                    B::SharedArrayBuffer
                } else {
                    B::ArrayBuffer
                };
                let byte_length = self.array_buffer_byte_length(value);
                (kind, value, 0, byte_length, byte_length)
            }
            Some(Cell::TypedArray { object, buffer, .. }) => {
                let (kind, buffer, offset, width) = (
                    super::embedding::typed_array_brand(object.native.kind()),
                    *buffer,
                    object.offset,
                    object.native.kind().width(),
                );
                let length = self.typed_array_length(value).unwrap_or(0);
                (kind, buffer, offset, length * width, length)
            }
            Some(Cell::DataView { .. }) => {
                let Some(Cell::DataView { object, buffer, .. }) = self.heap.get(value) else {
                    unreachable!("matched DataView");
                };
                let (buffer, offset) = (*buffer, object.offset);
                let byte_length = self.embedding_view_bytes_length(value);
                (B::DataView, buffer, offset, byte_length, byte_length)
            }
            _ => return Ok(None),
        };
        // A naked pointer cannot carry the synchronized shared store's guard.
        // Reject this host boundary explicitly rather than copy or expose bytes
        // while other runtimes may access them concurrently.
        if matches!(self.heap.get(buffer),Some(Cell::ArrayBuffer{bytes,shared:true,..}) if bytes.shared_backing().is_some())
        {
            return Err(self.type_error(&program,"Raw pointer access to synchronized shared backing requires a native lifetime protocol".into()));
        }
        let base = match self.heap.get_mut(buffer) {
            Some(Cell::ArrayBuffer {
                bytes,
                detached: false,
                resizable,
                ..
            }) if !bytes.is_empty() => {
                let before = bytes.capacity();
                let address = if *resizable {
                    bytes.as_ptr_bytes() as usize
                } else {
                    bytes.pin_address()
                };
                let after = bytes.capacity();
                self.heap.adjust_external_bytes(before, after);
                address as *mut u8
            }
            _ => std::ptr::null_mut(),
        };
        let data = if base.is_null() {
            base
        } else {
            base.wrapping_add(byte_offset)
        };
        Ok(Some((
            crate::BufferStorage {
                kind,
                data,
                byte_offset,
                byte_length,
                length,
                buffer: root,
            },
            buffer,
        )))
    }

    fn array_buffer_byte_length(&self, buffer: Value) -> usize {
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer {
                bytes,
                detached: false,
                ..
            }) => bytes.len(),
            _ => 0,
        }
    }

    /// A DataView's current byte length, zero once detached or out of bounds.
    fn embedding_view_bytes_length(&self, view: Value) -> usize {
        let Some(Cell::DataView {
            object,
            buffer,
            length,
            ..
        }) = self.heap.get(view)
        else {
            return 0;
        };
        if self.array_buffer_detached(*buffer) {
            return 0;
        }
        length.within(object.offset, 1, self.array_buffer_byte_length(*buffer)).unwrap_or(0)
    }

    pub(crate) fn embedding_uint8_array(&mut self, bytes: &[u8]) -> Result<Value, JsError> {
        self.embedding_uint8_array_from(bytes.to_vec())
    }

    /// A Uint8Array over a new ArrayBuffer that owns `bytes`, without copying them.
    pub(crate) fn embedding_uint8_array_from(&mut self, bytes: Vec<u8>) -> Result<Value, JsError> {
        let length = bytes.len();
        let buffer = self.fixed_array_buffer_from(bytes.into(), false);
        let buffer_root = self.heap.root(buffer);
        let view = self.heap.alloc(Cell::TypedArray {
            object: Box::new(crate::heap::BufferViewObject {
                native: crate::heap::NativeView::typed(TypedArrayKind::Uint8),
                object: Self::empty_object(self.typed_array_proto(TypedArrayKind::Uint8)),
                offset: 0,
            }),
            buffer: self.heap.root_value(buffer_root).unwrap(),
            length: crate::heap::ViewLength::fixed(length),
        });
        self.heap.release_root(buffer_root);
        Ok(view)
    }

    pub(crate) fn embedding_view_bytes(&self, value: Value) -> Option<Vec<u8>> {
        self.embedding_view_bytes_prefix(value, usize::MAX)
    }

    pub(crate) fn embedding_bytes_prefix(&self, value: Value, maximum: usize) -> Option<Vec<u8>> {
        if let Some(Cell::ArrayBuffer {
            bytes, detached, ..
        }) = self.heap.get(value)
        {
            let length = bytes.len().min(maximum);
            return Some(if *detached {
                Vec::new()
            } else {
                bytes.read()[..length].to_vec()
            });
        }
        self.embedding_view_bytes_prefix(value, maximum)
    }

    /// The backing buffer of an ArrayBuffer or view and the byte range it currently covers.
    pub(in crate::vm) fn embedding_buffer_range(&self, value: Value) -> Option<(Value, std::ops::Range<usize>)> {
        let (buffer, offset, byte_length) = match self.heap.get(value)? {
            Cell::ArrayBuffer { bytes, .. } => (value, 0, bytes.len()),
            Cell::TypedArray { object, buffer, .. } => {
                (*buffer, object.offset, self.typed_array_length(value)?.checked_mul(object.native.kind().width())?)
            }
            Cell::DataView { object, buffer, .. } => (*buffer, object.offset, self.embedding_view_bytes_length(value)),
            _ => return None,
        };
        Some((buffer, offset..offset.checked_add(byte_length)?))
    }

    /// Run `f` over an ArrayBuffer's or view's current bytes without copying them;
    /// empty once detached.
    pub(crate) fn embedding_with_buffer_bytes<T>(&self, value: Value, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
        let (buffer, range) = self.embedding_buffer_range(value)?;
        let Cell::ArrayBuffer { bytes, detached, .. } = self.heap.get(buffer)? else {
            return None;
        };
        if *detached {
            return Some(f(&[]));
        }
        let bytes = bytes.read();
        Some(f(bytes.get(range)?))
    }

    /// Run `f` over an ArrayBuffer's or view's current bytes for writing. `None` for
    /// another value, or a detached or immutable buffer, which accept no writes.
    pub(crate) fn embedding_with_buffer_bytes_mut<T>(&mut self, value: Value, f: impl FnOnce(&mut [u8]) -> T) -> Option<T> {
        let (buffer, range) = self.embedding_buffer_range(value)?;
        let Cell::ArrayBuffer { bytes, detached: false, immutable: false, .. } = self.heap.buffer_bytes_mut(buffer)? else {
            return None;
        };
        let mut bytes = bytes.make_mut();
        Some(f(bytes.get_mut(range)?))
    }

    fn embedding_view_bytes_prefix(&self, value: Value, maximum: usize) -> Option<Vec<u8>> {
        self.embedding_with_view_bytes(value, |bytes| bytes[..bytes.len().min(maximum)].to_vec())
    }

    /// Run `f` over the bytes a typed array or DataView currently views, without copying
    /// them; empty once detached.
    pub(crate) fn embedding_with_view_bytes<T>(&self, value: Value, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
        if matches!(self.heap.get(value)?, Cell::ArrayBuffer { .. }) {
            return None;
        }
        self.embedding_with_buffer_bytes(value, f)
    }

    /// Run `f` over the bytes `within` a view and a writable buffer's bytes at once: the input read
    /// in place unless a write to the output could reach it; `None` past the view's end.
    pub(crate) fn embedding_with_view_and_buffer_bytes_mut<T>(&mut self, input: Value, within: std::ops::Range<usize>, output: Value, f: impl FnOnce(&[u8], &mut [u8]) -> T) -> Option<T> {
        if matches!(self.heap.get(input)?, Cell::ArrayBuffer { .. }) {
            return None;
        }
        let (buffer, range) = self.embedding_buffer_range(input)?;
        let Cell::ArrayBuffer { bytes, detached, .. } = self.heap.get(buffer)? else { return None };
        let length = if *detached { 0 } else { range.len() };
        if within.start > within.end || within.end > length {
            return None;
        }
        let within = range.start + within.start..range.start + within.end;
        if within.is_empty() {
            return self.embedding_with_buffer_bytes_mut(output, |to| f(&[], to));
        }
        let Some(held) = bytes.unaliased_handle() else {
            let copy = bytes.read().get(within)?.to_vec();
            return self.embedding_with_buffer_bytes_mut(output, |to| f(&copy, to));
        };
        let from = held.read();
        let from = from.get(within)?;
        self.embedding_with_buffer_bytes_mut(output, |to| f(from, to))
    }
}
