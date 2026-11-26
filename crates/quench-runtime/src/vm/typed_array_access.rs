use super::*;
use crate::staged::typed_elements::{
    element_integer, element_number, find_number_element, store_double_element, store_element_number,
    native_word, store_number_element, NATIVE_LITTLE_ENDIAN,
};

const MAX_TYPED_ARRAY_ELEMENT_WIDTH: usize = std::mem::size_of::<u64>();

pub(super) enum TypedArrayElement {
    /// A BigInt element's value modulo 2^64, two's complement (ToBigInt64 / ToBigUint64).
    BigInt(u64),
    Number(f64),
}

impl Vm {
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
    pub(super) fn embedding_buffer_range(&self, value: Value) -> Option<(Value, std::ops::Range<usize>)> {
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

    /// The kind of the typed array `object`, which picks a site's kind variant
    /// (`typed_element_quick`).
    pub fn typed_array_kind_of(&self, object: Value) -> Option<TypedArrayKind> {
        match self.heap.get(object) {
            Some(Cell::TypedArray { object, .. }) => Some(object.native.kind()),
            _ => None,
        }
    }

    /// `typed_array_get` of a non-BigInt typed array, which allocates nothing: the element,
    /// or `undefined` past the end or of a detached buffer. One read of the view's cell, and
    /// an integer element's value made as an integer, so an index loop over a `Uint8Array`
    /// (ws's frame masking) converts through no float. `None` for any other object.
    #[inline]
    pub(super) fn typed_array_number_element(&self, object: Value, index: usize) -> Option<Value> {
        let view = self.typed_array_view(object)?;
        let kind = view.kind;
        if kind.is_bigint() {
            return None;
        }
        let Some(word) = self.typed_field(&view, index) else { return Some(Value::UNDEFINED) };
        let field = &word[..kind.width()];
        Some(match element_integer(kind, field) {
            Some(integer) => Value::integer(integer),
            None => Value::number(element_number(kind, field, NATIVE_LITTLE_ENDIAN)),
        })
    }

    /// A lane loop's view of the typed array `object`'s elements: its first element's address,
    /// element count and kind, which native code reads (and, `writable`, replaces) in place
    /// until the loop calls out. Only an attached buffer of Number elements this heap holds
    /// qualifies, and to write, one the buffer owns alone (`BufferBytes::mut_in_lane`), so no
    /// store copies or moves it; anything else stays on the general path.
    pub fn typed_array_elements_in_lane(&mut self, object: Value, writable: bool) -> Option<(*mut u8, usize, TypedArrayKind)> {
        let view = self.typed_array_view(object)?;
        if view.kind.is_bigint() || view.kind == TypedArrayKind::Float16 {
            return None;
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get_mut(view.buffer) else {
            return None;
        };
        let (base, byte_length) = if writable {
            let store = bytes.mut_in_lane()?;
            (store.as_mut_ptr(), store.len())
        } else {
            match bytes.read() {
                crate::heap::BufferRead::Borrowed(store) => (store.as_ptr().cast_mut(), store.len()),
                _ => return None,
            }
        };
        let length = view.length_within(byte_length);
        Some((base.wrapping_add(if length == 0 { 0 } else { view.offset }), length, view.kind))
    }

    /// The Number at `index` of the typed array `object` of `kind`, read by the staged element row
    /// over the view's native record (`typed_elements`), the same loads native code makes; the
    /// VM publishes the record when the proof asks for it. `None` for a kind with no native
    /// element, a view that cannot be addressed or an index past its elements, which the
    /// general read answers.
    fn staged_typed_number(&mut self, object: Value, kind: TypedArrayKind, index: usize) -> Option<f64> {
        crate::staged::typed_elements::NativeKind::new(kind)?;
        let directory = self.heap.typed_view_directory();
        let prove = || crate::staged::typed_view::concrete_typed_view(directory, object);
        let view = prove().or_else(|| self.publish_typed_view(object).then(prove).flatten())?;
        crate::staged::typed_view::concrete_typed_read(view, index).map(f64::from_bits)
    }

    /// The slow arm of the staged typed view proof: publishes the elements of the typed array or DataView
    /// `object` in its record's native prefix as its buffer is now, which stays true until the next buffer
    /// event (`buffer_relocations`); a view native code cannot address is withdrawn instead. Whether it is
    /// addressable.
    pub fn publish_typed_view(&mut self, object: Value) -> bool {
        if matches!(self.heap.get(object), Some(Cell::TypedArray { object, .. } | Cell::DataView { object, .. }) if object.native.is_retracted()) {
            return false;
        }
        let unshared = self.view_elements_in_lane(object, true);
        let published = unshared.or_else(|| self.view_elements_in_lane(object, false))
            .and_then(|(data, length)| Some((data, u32::try_from(i32::try_from(length).ok()?).ok()?)));
        let record = match self.heap.get_mut(object) {
            Some(Cell::TypedArray { object, .. } | Cell::DataView { object, .. }) => object,
            _ => return false,
        };
        match published {
            Some((data, length)) => record.native.publish(data, length, unshared.is_some()),
            None => record.native.retract(),
        }
        published.is_some()
    }

    /// The elements of the typed array or DataView `object` as native code reads them (`publish_typed_view`): the first
    /// element's address and their count, over bytes that `writable` lets it store to.
    fn view_elements_in_lane(&mut self, object: Value, writable: bool) -> Option<(*mut u8, usize)> {
        if matches!(self.heap.get(object), Some(Cell::DataView { .. })) {
            return self.data_view_elements_in_lane(object, writable);
        }
        self.typed_array_elements_in_lane(object, writable).map(|(data, length, _)| (data, length))
    }

    /// The bytes of the DataView `object` as native code reads them: its view's first byte and its byte length, for a view of
    /// fixed length that lies inside its buffer's bytes. Over a store, the buffer must be mutable and owned alone
    /// (`BufferBytes::mut_in_lane`). A length-tracking view or one out of its buffer's bounds has no such elements.
    fn data_view_elements_in_lane(&mut self, object: Value, writable: bool) -> Option<(*mut u8, usize)> {
        let Some(Cell::DataView { buffer, object: view, length }) = self.heap.get(object) else { return None };
        let (buffer, offset, length) = (*buffer, view.offset, length.get()?);
        let end = offset.checked_add(length)?;
        let Some(Cell::ArrayBuffer { bytes, detached: false, immutable, .. }) = self.heap.get_mut(buffer) else {
            return None;
        };
        let (base, byte_length) = if writable {
            if *immutable { return None; }
            let store = bytes.mut_in_lane()?;
            (store.as_mut_ptr(), store.len())
        } else {
            match bytes.read() {
                crate::heap::BufferRead::Borrowed(store) => (store.as_ptr().cast_mut(), store.len()),
                _ => return None,
            }
        };
        (end <= byte_length).then(|| (base.wrapping_add(offset), length))
    }

    /// `get_index` of a BigInt typed array at an index key (the BigInt it allocates); `None` for any other read.
    pub fn bigint_typed_element(&mut self, object: Value, key: Value) -> Option<Value> {
        // One cell read decides the kind; a Number kind's or any other object's read declines here.
        let view = self.typed_array_view(object).filter(|view| view.kind.is_bigint())?;
        let index = self.array_index_key(key)? as usize;
        let Some(bytes) = self.typed_field(&view, index) else { return Some(Value::UNDEFINED) };
        Some(self.bigint_element(view.kind, bytes))
    }

    /// A BigInt kind's element word as its one-digit BigInt, made in place: no `num_bigint` value and its
    /// digit vector.
    #[inline(always)]
    fn bigint_element(&mut self, kind: TypedArrayKind, bytes: [u8; 8]) -> Value {
        let (word, signed) = (u64::from_ne_bytes(bytes), kind == TypedArrayKind::BigInt64);
        let negative = signed && (word as i64) < 0;
        self.heap.alloc(Cell::BigInt(crate::bigint::JsBigInt::from_digits(negative, &[if negative { (word as i64).unsigned_abs() } else { word }])))
    }

    pub(super) fn typed_array_get(&mut self, object: Value, index: usize) -> Option<Value> {
        let view = self.typed_array_view(object)?;
        let kind = view.kind;
        if let Some(number) = self.staged_typed_number(object, kind, index) {
            return Some(Value::number(number));
        }
        let Some(bytes) = self.typed_field(&view, index) else { return Some(Value::UNDEFINED) };
        Some(if kind.is_bigint() {
            self.bigint_element(kind, bytes)
        } else {
            Value::number(element_number(kind, &bytes[..kind.width()], NATIVE_LITTLE_ENDIAN))
        })
    }

    /// The first index in `from..end` of the typed array `object` whose element is the Number
    /// `search` (strictly, or under `nan_matches` as SameValueZero), read over its attached bytes:
    /// the search core `staged::typed_elements::find_number_element` over the elements the
    /// general element read would answer. An element past the view's length or of a detached
    /// buffer matches nothing, as `typed_array_get` answers `undefined` for it.
    pub(super) fn typed_array_find_number(
        &self,
        object: Value,
        from: usize,
        end: usize,
        search: f64,
        nan_matches: bool,
    ) -> Option<usize> {
        let view = self.typed_array_view(object)?;
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(view.buffer) else {
            return None;
        };
        let bytes = bytes.read();
        let end = end.min(view.length_within(bytes.len()));
        let elements = bytes.get(view.offset..view.offset + end * view.kind.width())?;
        find_number_element(elements, view.kind, from, search, nan_matches)
    }

    pub(super) fn typed_array_length(&self, object: Value) -> Option<usize> {
        let (buffer, offset, length, kind) = match self.heap.get(object) {
            Some(Cell::TypedArray { buffer, object, length }) => (*buffer, object.offset, *length, object.native.kind()),
            _ => return None,
        };
        Some(match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, detached: false, .. }) => {
                length.within(offset, kind.width(), bytes.len()).unwrap_or(0)
            }
            _ => 0,
        })
    }

    pub(super) fn typed_array_length_is_variable(&self, object: Value) -> bool {
        let Some(Cell::TypedArray { buffer, length, .. }) = self.heap.get(object) else {
            return false;
        };
        matches!(
            self.heap.get(*buffer),
            Some(Cell::ArrayBuffer {
                shared,
                resizable: true,
                ..
            }) if !shared || length.is_tracking()
        )
    }

    pub(super) fn typed_array_out_of_bounds(&self, object: Value) -> bool {
        let Some(Cell::TypedArray {
            buffer,
            object,
            length,
            ..
        }) = self.heap.get(object)
        else {
            return false;
        };
        match self.heap.get(*buffer) {
            Some(Cell::ArrayBuffer { bytes, detached: false, .. }) => {
                length.within(object.offset, object.native.kind().width(), bytes.len()).is_none()
            }
            _ => true,
        }
    }

    pub(super) fn typed_array_shared(&self, object: Value) -> Option<bool> {
        let buffer = match self.heap.get(object) {
            Some(Cell::TypedArray { buffer, .. }) => *buffer,
            _ => return None,
        };
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { shared, .. }) => Some(*shared),
            _ => None,
        }
    }

    pub fn typed_array_kind(&self, object: Value) -> Option<TypedArrayKind> {
        match self.heap.get(object) {
            Some(Cell::TypedArray { object, .. }) => Some(object.native.kind()),
            _ => None,
        }
    }

    pub(super) fn typed_array_byte_offset(&self, object: Value) -> Option<usize> {
        let offset = match self.heap.get(object) {
            Some(Cell::TypedArray { object, .. }) => object.offset,
            _ => return None,
        };
        Some(if self.typed_array_out_of_bounds(object) {
            0
        } else {
            offset
        })
    }

    pub(super) fn set_through_typed_array_prototype(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        atom: Atom,
        value: Value,
    ) -> Result<Option<bool>, JsError> {
        let name = self.atom_name(atom);
        let Some(number) = name
            .parse::<f64>()
            .ok()
            .filter(|number| crate::number_to_string::format(*number) == name || name == "-0")
        else {
            return Ok(None);
        };
        let index = name
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == name && number == *index as f64);
        let key = self.heap.alloc_string(name.into());
        let mut current = self
            .object_data(receiver)
            .map_or(Value::NULL, |data| data.proto());
        let mut visited = std::collections::HashSet::new();
        while !current.is_null() && visited.insert(current) {
            match self.heap.get(current) {
                Some(Cell::TypedArray { .. }) => {
                    return match index {
                        Some(index) => self.typed_array_set(p, current, index, value).map(Some),
                        None => Ok(Some(false)),
                    };
                }
                Some(Cell::Proxy { .. }) => return Ok(None),
                _ => {}
            }
            if !self
                .object_get_own_property_descriptor(p, &[current, key])?
                .is_undefined()
            {
                return Ok(None);
            }
            current = self.object_get_prototype_of(p, current)?;
        }
        Ok(None)
    }

    pub(super) fn typed_array_set(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
    ) -> Result<bool, JsError> {
        let Some(kind) = self.typed_array_kind(object) else {
            return Ok(false);
        };
        if self.typed_array_set_number(object, index, value) {
            return Ok(true);
        }
        let object = self.heap.root(object);
        let outcome = (|| {
            let value = self.typed_array_convert_value(p, kind, value)?;
            let object = self.heap.root_value(object).unwrap();
            Ok(self.typed_array_write_element(object, index, &value))
        })();
        self.heap.release_root(object);
        outcome
    }

    /// `typed_array_set` of a Number into a typed array of Number content: its ToNumber has no
    /// effect, so the write needs neither a root nor a conversion. False leaves the store to
    /// `typed_array_set`.
    pub fn typed_array_set_number(&mut self, object: Value, index: usize, value: Value) -> bool {
        if let Some(integer) = value.as_int()
            && self.typed_array_set_integer(object, index, integer)
        {
            return true;
        }
        let Some(number) = value.as_number() else {
            return false;
        };
        match self.typed_array_view(object) {
            Some(view) if !view.kind.is_bigint() => {
                self.typed_array_write_view(view, index, &TypedArrayElement::Number(number));
                true
            }
            _ => false,
        }
    }

    /// `typed_array_set_number` of an integer into an integer typed array whose buffer the
    /// view owns alone (`BufferBytes::mut_in_lane`): an integer's ToInt8 through ToUint32 is its low
    /// bytes, and Uint8Clamped clamps it. False leaves the store to the general write.
    #[inline]
    fn typed_array_set_integer(&mut self, object: Value, index: usize, integer: i32) -> bool {
        let Some(view) = self.typed_array_view(object) else { return false };
        let width = match view.kind {
            TypedArrayKind::Uint8Clamped | TypedArrayKind::Uint8 | TypedArrayKind::Int8 => size_of::<u8>(),
            TypedArrayKind::Uint16 | TypedArrayKind::Int16 => size_of::<u16>(),
            TypedArrayKind::Uint32 | TypedArrayKind::Int32 => size_of::<u32>(),
            _ => return false,
        };
        let Some(Cell::ArrayBuffer { bytes: store, detached: false, resizable, .. }) = self.heap.buffer_bytes_mut(view.buffer) else {
            return false;
        };
        let Some(store) = store.mut_in_lane() else { return false };
        // A fixed view of a fixed buffer is in bounds by its length; past it nothing is written.
        let start = match view.length.get() {
            Some(length) if !*resizable => (index < length).then(|| view.offset + index * width),
            _ => view.element(index, store.len()),
        };
        if let Some(start) = start {
            // SAFETY: `start` is an element of `view` in the store, so its `width` bytes are in bounds
            // (the slice checks them), and an integer kind's element store takes the integer.
            unsafe { store_number_element(view.kind, store[start..start + width].as_mut_ptr(), Value::integer(integer)) };
        }
        true
    }

    pub(super) fn typed_array_write_element(
        &mut self,
        object: Value,
        index: usize,
        value: &TypedArrayElement,
    ) -> bool {
        let Some(view) = self.typed_array_view(object) else {
            return false;
        };
        self.typed_array_write_view(view, index, value);
        true
    }

    /// Writes element `index` of the typed array `view` describes; an index past its
    /// current length, or a detached or shrunken buffer, writes nothing.
    fn typed_array_write_view(&mut self, view: TypedArrayView, index: usize, value: &TypedArrayElement) {
        let encoded = Self::typed_array_element_bytes(view.kind, value);
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.buffer_bytes_mut(view.buffer) else {
            return;
        };
        // An owned store no other cell shares is written in place (`BufferBytes::mut_in_lane`);
        // any other is unshared or locked first.
        if let Some(store) = bytes.mut_in_lane() {
            if let Some(start) = view.element(index, store.len()) {
                store_element(store, start, view.kind.width(), &encoded);
            }
            return;
        }
        if let Some(start) = view.element(index, bytes.len()) {
            store_element(&mut bytes.make_mut(), start, view.kind.width(), &encoded);
        }
    }

    /// Whether `value` is a typed array, whose elements a loop region reads in place
    /// (`typed_array_elements_in_lane`) and a function body has no row for.
    pub fn is_typed_array(&self, value: Value) -> bool {
        self.typed_array_view(value).is_some()
    }

    /// What an element access reads of the typed array `object` before its buffer: one heap
    /// read, where `typed_array_length` and the buffer checks would each make their own.
    /// The bytes of `view`'s element at `index` in an eight-byte word, `None` past the end or of a detached buffer. A store
    /// this heap owns is read in place (`BufferBytes::owned`); a shared one through its lock.
    #[inline]
    fn typed_field(&self, view: &TypedArrayView, index: usize) -> Option<[u8; 8]> {
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(view.buffer) else { return None };
        let read = |bytes: &[u8]| view.element(index, bytes.len()).map(|start| native_word(&bytes[start..start + view.kind.width()], NATIVE_LITTLE_ENDIAN));
        match bytes.owned() { Some(owned) => read(owned), None => read(&bytes.read()) }
    }

    fn typed_array_view(&self, object: Value) -> Option<TypedArrayView> {
        match self.heap.get(object) {
            Some(Cell::TypedArray { object, buffer, length }) => Some(TypedArrayView {
                kind: object.native.kind(),
                buffer: *buffer,
                offset: object.offset,
                length: *length,
            }),
            _ => None,
        }
    }

    /// One element encoding serves indexed stores and validated bulk fills.
    #[inline]
    fn typed_array_element_bytes(
        kind: TypedArrayKind,
        value: &TypedArrayElement,
    ) -> [u8; MAX_TYPED_ARRAY_ELEMENT_WIDTH] {
        // A Number element is its kind's word (`element_word`): the first `width` bytes, the rest zero.
        if let TypedArrayElement::Number(number) = value
            && !kind.is_bigint()
        {
            return crate::staged::typed_elements::element_word(kind, *number);
        }
        let mut bytes = [0; MAX_TYPED_ARRAY_ELEMENT_WIDTH];
        let number = match value {
            TypedArrayElement::Number(value) => *value,
            _ => 0.0,
        };
        match kind {
            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                let TypedArrayElement::BigInt(value) = value else {
                    unreachable!("BigInt content type was validated");
                };
                bytes = value.to_le_bytes();
            }
            _ => store_element_number(kind, number, &mut bytes[..kind.width()], NATIVE_LITTLE_ENDIAN),
        }
        bytes
    }

    /// Coercion and live-view checks precede this effect-free non-shared fast path.
    pub(super) fn typed_array_fill_elements(
        &mut self,
        object: Value,
        start: usize,
        end: usize,
        value: &TypedArrayElement,
    ) -> bool {
        if start >= end {
            return true;
        }
        let Some(Cell::TypedArray { object: view, buffer, length }) = self.heap.get(object) else {
            return false;
        };
        let view = TypedArrayView {
            kind: view.native.kind(),
            buffer: *buffer,
            offset: view.offset,
            length: *length,
        };
        self.fill_view_elements(view, start, end, value)
    }

    /// `typed_array_fill_elements` on a view already read: one buffer read for the checks (attached,
    /// not shared, the view's length covering `end`), one encoding of the value, one buffer write.
    /// A 1-byte kind fills with a memset; a wider one with its encoded element repeated by doubling
    /// `copy_within` copies, so a long fill makes O(log n) copy calls rather than one per element.
    fn fill_view_elements(&mut self, view: TypedArrayView, start: usize, end: usize, value: &TypedArrayElement) -> bool {
        let width = view.kind.width();
        let Some(Cell::ArrayBuffer { bytes, detached: false, shared: false, .. }) = self.heap.get(view.buffer) else {
            return false;
        };
        let Some(elements) = view.length.within(view.offset, width, bytes.len()) else {
            return false;
        };
        if end > elements {
            return false;
        }
        let encoded = Self::typed_array_element_bytes(view.kind, value);
        let element = &encoded[..width];
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(view.buffer) else {
            return false;
        };
        let mut store = bytes.make_mut();
        let Some(span) = store.get_mut(view.offset + start * width..view.offset + end * width) else {
            return false;
        };
        if width == 1 || element.iter().all(|byte| *byte == element[0]) {
            span.fill(element[0]);
        } else {
            span[..width].copy_from_slice(element);
            let mut filled = width;
            while filled < span.len() {
                let count = filled.min(span.len() - filled);
                span.copy_within(..count, filled);
                filled += count;
            }
        }
        true
    }

    /// Copies validated views without guest effects; callers retain roots for both views.
    pub(super) fn typed_array_copy_elements(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        target: Value,
        target_start: usize,
        length: usize,
    ) -> Result<(), JsError> {
        let Some(Cell::TypedArray { buffer, object, .. }) = self.heap.get(source) else {
            unreachable!("typed copy source was validated");
        };
        let (source_kind, source_buffer, source_offset) = (object.native.kind(), *buffer, object.offset);
        let Some(Cell::TypedArray { buffer, object, .. }) = self.heap.get(target) else {
            unreachable!("typed copy target was validated");
        };
        let (target_kind, target_buffer, target_offset) = (object.native.kind(), *buffer, object.offset);
        if source_kind.is_bigint() != target_kind.is_bigint() {
            return Err(self.type_error(p, super::operations::MIXED_BIGINT_TYPES.into()));
        }
        if source_kind == target_kind {
            let target_offset = target_offset + target_start * target_kind.width();
            let byte_length = length * source_kind.width();
            if self.array_buffers_share_data_block(source_buffer, target_buffer) {
                let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(target_buffer) else {
                    unreachable!("typed copy backing buffer was validated");
                };
                // Set snapshots overlapping source bytes, unlike slice's forward copy.
                if let Some(backing) = bytes.shared_backing() {
                    // Growth can leave source and target with different visible prefixes
                    // of the same data block. The source range was checked on its view.
                    backing.copy_from(&backing, source_offset..source_offset + byte_length,
                        target_offset..target_offset + byte_length);
                } else {
                    bytes.make_mut()
                        .copy_within(source_offset..source_offset + byte_length, target_offset);
                }
            } else {
                let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(source_buffer) else {
                    unreachable!("typed copy source backing buffer was validated");
                };
                let source_bytes = bytes.read()[source_offset..source_offset + byte_length].to_vec();
                let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(target_buffer) else {
                    unreachable!("typed copy target backing buffer was validated");
                };
                bytes.make_mut()[target_offset..target_offset + byte_length]
                    .copy_from_slice(&source_bytes);
            }
            return Ok(());
        }
        if !source_kind.is_bigint()
            && self.typed_array_convert_numbers(source_buffer, source_offset, source_kind, target_buffer, target_offset + target_start * target_kind.width(), target_kind, length)
        {
            return Ok(());
        }
        let snapshot = if self.array_buffers_share_data_block(source_buffer,target_buffer) {
            let mut values = Vec::with_capacity(length);
            for index in 0..length {
                let value = self.typed_array_get(source, index).unwrap();
                values.push(self.typed_array_convert_value(p, target_kind, value)?);
            }
            Some(values)
        } else {
            None
        };
        for index in 0..length {
            let converted;
            let value = if let Some(values) = &snapshot {
                &values[index]
            } else {
                let value = self.typed_array_get(source, index).unwrap();
                converted = self.typed_array_convert_value(p, target_kind, value)?;
                &converted
            };
            self.typed_array_write_element(target, target_start + index, value);
        }
        Ok(())
    }

    /// `typed_array_copy_elements` of `length` Number elements of another kind, made on the
    /// bytes: each source element (`staged::typed_elements::element_number`) converted as its
    /// write converts a Number (`staged::typed_elements::element_word`), a run reading the source's bytes before writing
    /// any, as the spec's clone of a shared source does. `false`, writing nothing, where either
    /// range lies past its buffer, which the element copy then decides.
    #[allow(clippy::too_many_arguments)]
    fn typed_array_convert_numbers(
        &mut self,
        source_buffer: Value,
        source_offset: usize,
        source_kind: TypedArrayKind,
        target_buffer: Value,
        target_offset: usize,
        target_kind: TypedArrayKind,
        length: usize,
    ) -> bool {
        let (source_width, target_width) = (source_kind.width(), target_kind.width());
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(source_buffer) else {
            return false;
        };
        let source_end = length.checked_mul(source_width).and_then(|bytes| bytes.checked_add(source_offset));
        let Some(source) = source_end.and_then(|end| bytes.read().get(source_offset..end).map(<[u8]>::to_vec)) else {
            return false;
        };
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get_mut(target_buffer) else {
            return false;
        };
        let target_end = length.checked_mul(target_width).and_then(|bytes| bytes.checked_add(target_offset));
        let Some(target_end) = target_end.filter(|end| *end <= bytes.len()) else {
            return false;
        };
        let mut target = bytes.make_mut();
        let target = &mut target[target_offset..target_end];
        Self::typed_array_convert_elements(&source, source_kind, target, target_kind);
        true
    }

    pub(super) fn typed_array_convert_value(
        &mut self,
        p: &ResidualProgram,
        kind: TypedArrayKind,
        value: Value,
    ) -> Result<TypedArrayElement, JsError> {
        match kind {
            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                // A BigInt primitive needs no ToPrimitive and no num_bigint copy.
                if let Some(Cell::BigInt(value)) = self.heap.get(value) {
                    return Ok(TypedArrayElement::BigInt(value.wrapping_u64()));
                }
                let value = self.to_bigint(p, value)?;
                let low = value.iter_u64_digits().next().unwrap_or(0);
                let bits = if value.sign() == num_bigint::Sign::Minus { low.wrapping_neg() } else { low };
                Ok(TypedArrayElement::BigInt(bits))
            }
            _ => self.to_number(p, value).map(TypedArrayElement::Number),
        }
    }
}

mod element_conversion;

#[cfg(test)]
mod backing_address_tests;

/// A typed array's view of its buffer, as an element access reads it (`Vm::typed_array_view`).
#[derive(Clone, Copy)]
struct TypedArrayView {
    kind: TypedArrayKind,
    buffer: Value,
    offset: usize,
    length: crate::heap::ViewLength,
}

impl TypedArrayView {
    /// The elements an attached buffer of `byte_length` bytes holds, as
    /// `Vm::typed_array_length` derives them: a length-tracking view covers the buffer past its
    /// offset, a fixed one covers none of it once the buffer shrank below its end.
    #[inline(always)]
    fn length_within(self, byte_length: usize) -> usize {
        self.length.within(self.offset, self.kind.width(), byte_length).unwrap_or(0)
    }

    /// The byte offset of element `index` in an attached buffer of `byte_length` bytes, or
    /// `None` past the array's length (`length_within`).
    #[inline(always)]
    fn element(self, index: usize, byte_length: usize) -> Option<usize> {
        (index < self.length_within(byte_length)).then(|| self.offset + index * self.kind.width())
    }
}

/// Writes the first `width` bytes of `encoded` at `start` of `store`, as fixed-width moves:
/// a slice copy of the element's runtime width is a `memcpy` call.
#[inline(always)]
fn store_element(store: &mut [u8], start: usize, width: usize, encoded: &[u8; MAX_TYPED_ARRAY_ELEMENT_WIDTH]) {
    fn fixed<const WIDTH: usize>(store: &mut [u8], start: usize, encoded: &[u8; MAX_TYPED_ARRAY_ELEMENT_WIDTH]) {
        let source: &[u8; WIDTH] = encoded[..WIDTH].try_into().expect("an element fits the encoding");
        store[start..start + WIDTH].copy_from_slice(source);
    }
    match width {
        1 => store[start] = encoded[0],
        2 => fixed::<2>(store, start, encoded),
        4 => fixed::<4>(store, start, encoded),
        _ => fixed::<MAX_TYPED_ARRAY_ELEMENT_WIDTH>(store, start, encoded),
    }
}

/// `typed_element_quick`'s `KIND` for a site that has seen more than one kind.
pub const ANY_TYPED_KIND: usize = usize::MAX;

/// `kind` where it is the site's `KIND`, as a constant the element access is compiled for;
/// any kind under `ANY_TYPED_KIND`.
#[inline(always)]
fn typed_kind<const KIND: usize>(kind: TypedArrayKind) -> Option<TypedArrayKind> {
    if KIND == ANY_TYPED_KIND {
        return Some(kind);
    }
    let constant = crate::heap::TYPED_ARRAYS[KIND % TypedArrayKind::COUNT].kind;
    (kind == constant).then_some(constant)
}

/// `store_number_element` of any Number, a double into an integer kind by `store_double_element`
/// (both in `staged::typed_elements`).
///
/// # Safety
/// As `store_number_element`.
#[inline(always)]
unsafe fn store_any_number(kind: TypedArrayKind, target: *mut u8, value: Value) -> bool {
    // SAFETY: the caller's contract.
    unsafe { store_number_element(kind, target, value) || store_double_element(kind, target, value) }
}

/// How many views `TypedViewCache` holds, a power of two: a view's slot index picks its entry.
const TYPED_VIEW_ENTRIES: usize = 64;

/// The element addresses of recently accessed typed array views, by view: what a lane
/// element access reads after one compare of the view and one of the epoch, where the general
/// access reads the view's cell and its buffer's cell (`Vm::typed_cached_element`). An entry
/// holds while the view epoch it was made at is current (`heap::view_epoch`), which every
/// event that could move, unshare, detach or resize a store, or reuse a slot, ends.
pub(super) struct TypedViewCache([TypedViewEntry; TYPED_VIEW_ENTRIES]);

impl Default for TypedViewCache {
    fn default() -> Self {
        Self([TypedViewEntry::EMPTY; TYPED_VIEW_ENTRIES])
    }
}

#[derive(Clone, Copy)]
struct TypedViewEntry {
    /// The view's value bits.
    view: u64,
    /// The view epoch the entry was made at.
    epoch: u64,
    /// The view's first element.
    elements: *mut u8,
    /// The view's element count, below 2^31 (`Value::int_key_offset`); zero for an empty entry.
    length: usize,
    kind: TypedArrayKind,
    /// Whether the view's buffer is owned alone and mutable, so stores write `elements`.
    writable: bool,
}

impl TypedViewEntry {
    const EMPTY: Self = Self {
        view: 0,
        epoch: u64::MAX,
        elements: std::ptr::null_mut(),
        length: 0,
        kind: TypedArrayKind::Uint8,
        writable: false,
    };
}

impl TypedViewCache {
    #[inline(always)]
    fn entry(&self, view: Value) -> &TypedViewEntry {
        &self.0[view.to_bits() as usize % TYPED_VIEW_ENTRIES]
    }
}

impl Vm {
    /// The element count of the typed array `object` from its current cached entry
    /// (`TypedViewCache`): what its intrinsic `length` getter answers, where the entry holds,
    /// since every event that could detach, resize or move its buffer ends the view epoch.
    #[inline(always)]
    pub(super) fn typed_cached_length(&self, object: Value) -> Option<Value> {
        let entry = self.typed_views.entry(object);
        // `fill_typed_view` clamps the count to `i32::MAX`: a longer view takes the full read.
        (entry.view == object.to_bits() && entry.epoch == crate::heap::view_epoch() && entry.length < i32::MAX as usize)
            .then(|| Value::length(entry.length))
    }

    /// The element of the typed array `object` at `key` from its cached entry
    /// (`TypedViewCache`), when `object` has a current one of kind `KIND` (or any kind) and
    /// `key` is an int32 inside it. `None` otherwise; the caller fills the entry
    /// (`fill_typed_view`) on its slower path.
    #[inline(always)]
    pub(super) fn typed_cached_element<const KIND: usize>(&self, object: Value, key: Value) -> Option<Value> {
        let entry = self.typed_views.entry(object);
        if entry.view != object.to_bits() || entry.epoch != crate::heap::view_epoch() {
            return None;
        }
        let kind = typed_kind::<KIND>(entry.kind)?;
        let index = key.int_key_offset();
        if index >= entry.length {
            return None;
        }
        // SAFETY: the entry is current, so `elements` holds `length` elements of `kind`
        // (`fill_typed_view`), and `index` is below `length`.
        let element = unsafe { entry.elements.add(index * kind.width()) };
        macro_rules! element {
            ($type:ty) => {
                // SAFETY: as above.
                unsafe { element.cast::<$type>().read_unaligned() }
            };
        }
        Some(match kind {
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => Value::integer(i32::from(element!(u8))),
            TypedArrayKind::Int8 => Value::integer(i32::from(element!(i8))),
            TypedArrayKind::Uint16 => Value::integer(i32::from(element!(u16))),
            TypedArrayKind::Int16 => Value::integer(i32::from(element!(i16))),
            TypedArrayKind::Int32 => Value::integer(element!(i32)),
            TypedArrayKind::Uint32 => {
                let element = element!(u32);
                match i32::try_from(element) {
                    Ok(integer) => Value::integer(integer),
                    Err(_) => Value::number(f64::from(element)),
                }
            }
            TypedArrayKind::Float32 => Value::number(f64::from(element!(f32))),
            TypedArrayKind::Float64 => Value::number(element!(f64)),
            _ => return None,
        })
    }

    /// A typed element store through the cached entry of `object` (as `typed_cached_element`), of
    /// a writable view: an integer, or any Number into a float kind. False leaves the store to
    /// the caller's slower path (`typed_cached_store_number` first).
    #[inline(always)]
    pub fn typed_cached_store<const KIND: usize>(&mut self, object: Value, key: Value, value: Value) -> bool {
        let Some((kind, element)) = self.typed_cached_slot::<KIND>(object, key) else { return false };
        // SAFETY: `typed_cached_slot` proved one element of `kind` writable at `element`.
        unsafe { store_number_element(kind, element, value) }
    }

    /// `typed_cached_store` of any Number, a double into an integer kind included
    /// (`store_double_element`): the store-miss handler's first try, before it refills the entry.
    #[inline(always)]
    pub fn typed_cached_store_number<const KIND: usize>(&mut self, object: Value, key: Value, value: Value) -> bool {
        let Some((kind, element)) = self.typed_cached_slot::<KIND>(object, key) else { return false };
        // SAFETY: as `typed_cached_store`.
        unsafe { store_any_number(kind, element, value) }
    }

    /// The kind and element address at `key` of the current, writable cached entry of
    /// `object`, a view of kind `KIND` (`typed_kind`); `None` otherwise.
    #[inline(always)]
    fn typed_cached_slot<const KIND: usize>(&mut self, object: Value, key: Value) -> Option<(TypedArrayKind, *mut u8)> {
        let entry = self.typed_views.entry(object);
        if entry.view != object.to_bits() || entry.epoch != crate::heap::view_epoch() || !entry.writable {
            return None;
        }
        let kind = typed_kind::<KIND>(entry.kind)?;
        let index = key.int_key_offset();
        if index >= entry.length {
            return None;
        }
        // SAFETY: the entry is current and writable, so `elements` holds `length` elements of
        // `kind` in a store this view's buffer owns alone (`fill_typed_view`).
        Some((kind, unsafe { entry.elements.add(index * kind.width()) }))
    }

    /// Makes the cached entry of the typed array `object` (`TypedViewCache`) at the current
    /// view epoch: a fixed length view of Number elements inside an attached buffer whose
    /// store this heap owns, writable when the buffer owns it alone and is mutable. Any other
    /// object leaves the cache as it is.
    #[inline(never)]
    pub fn fill_typed_view(&mut self, object: Value) {
        let Some(Cell::TypedArray { object: view, buffer, length }) = self.heap.get(object) else {
            return;
        };
        let (kind, offset, buffer) = (view.native.kind(), view.offset, *buffer);
        let Some(length) = length.get() else {
            return;
        };
        if kind.is_bigint() || kind == TypedArrayKind::Float16 {
            return;
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, immutable, .. }) = self.heap.buffer_bytes_mut(buffer) else {
            return;
        };
        let immutable = *immutable;
        let width = kind.width();
        let (elements, writable) = match bytes.mut_in_lane() {
            Some(store) if offset.saturating_add(length.saturating_mul(width)) <= store.len() => {
                // SAFETY: the view lies inside the store (checked just above).
                (unsafe { store.as_mut_ptr().add(offset) }, !immutable)
            }
            Some(_) => return,
            None => match bytes.owned() {
                Some(store) if offset.saturating_add(length.saturating_mul(width)) <= store.len() => {
                    // SAFETY: as above; read only (`writable` false).
                    (unsafe { store.as_ptr().add(offset) }.cast_mut(), false)
                }
                _ => return,
            },
        };
        let slot = object.to_bits() as usize % TYPED_VIEW_ENTRIES;
        self.typed_views.0[slot] = TypedViewEntry {
            view: object.to_bits(),
            epoch: crate::heap::view_epoch(),
            elements,
            length: length.min(i32::MAX as usize),
            kind,
            writable,
        };
    }
}

/// A loop kernel's view of a typed array's elements (`Vm::typed_array_span`).
#[derive(Clone, Copy)]
pub struct TypedSpan {
    /// The first element's byte.
    pub first: *mut u8,
    pub kind: TypedArrayKind,
    pub length: usize,
    /// Whether a store may write the elements in place: the buffer's bytes are this
    /// buffer's alone and mutable.
    pub writable: bool,
    /// The buffer's bytes, which two views over one buffer share.
    pub store: (usize, usize),
}

impl Vm {
    /// The elements of the fixed-length typed array `object` of a Number kind, as a loop
    /// kernel reads and writes them without guest code; `None` for any other value, a
    /// detached, length-tracking, shared or Wasm buffer, or a BigInt or Float16 kind. Valid
    /// while nothing allocates, detaches or resizes.
    pub fn typed_array_span(&mut self, object: Value) -> Option<TypedSpan> {
        let Some(Cell::TypedArray { object: view, buffer, length }) = self.heap.get(object) else {
            return None;
        };
        let (kind, offset, buffer, length) = (view.native.kind(), view.offset, *buffer, length.get()?);
        if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 | TypedArrayKind::Float16) {
            return None;
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, immutable, .. }) = self.heap.buffer_bytes_mut(buffer) else {
            return None;
        };
        let immutable = *immutable;
        let (store, writable) = match bytes.mut_in_lane().filter(|_| !immutable) {
            Some(store) => (store.as_mut_ptr(), store.len()),
            None => (bytes.owned()?.as_ptr().cast_mut(), usize::MAX),
        };
        let (store, store_length, writable) = if writable == usize::MAX {
            let length = bytes.owned()?.len();
            (store, length, false)
        } else {
            (store, writable, true)
        };
        if offset.checked_add(length.checked_mul(kind.width())?)? > store_length {
            return None;
        }
        // SAFETY: the view lies inside the store (checked above).
        let first = unsafe { store.add(offset) };
        Some(TypedSpan { first, kind, length, writable, store: (store as usize, store_length) })
    }
}
