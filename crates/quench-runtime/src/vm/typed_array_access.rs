use super::*;

const MAX_TYPED_ARRAY_ELEMENT_WIDTH: usize = std::mem::size_of::<u64>();

pub(super) enum TypedArrayElement {
    BigInt(num_bigint::BigInt),
    Number(f64),
}

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
                let kind = if *shared { B::SharedArrayBuffer } else { B::ArrayBuffer };
                let byte_length = self.array_buffer_byte_length(value);
                (kind, value, 0, byte_length, byte_length)
            }
            Some(Cell::TypedArray { object, buffer, kind, .. }) => {
                let (kind, buffer, offset, width) =
                    (super::embedding::typed_array_brand(*kind), *buffer, object.offset, kind.width());
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
        if matches!(self.heap.get(buffer),Some(Cell::ArrayBuffer{bytes,shared:true,..}) if bytes.shared_backing().is_some()) {
            return Err(self.type_error(&program,"Raw pointer access to synchronized shared backing requires a native lifetime protocol".into()));
        }
        let base = match self.heap.get_mut(buffer) {
            Some(Cell::ArrayBuffer { bytes, detached: false, resizable, .. }) if !bytes.is_empty() => {
                let before = bytes.capacity();
                let address = if *resizable { bytes.as_ptr_bytes() as usize } else { bytes.pin_address() };
                let after = bytes.capacity();
                self.heap.adjust_external_bytes(before, after);
                address as *mut u8
            }
            _ => std::ptr::null_mut(),
        };
        let data = if base.is_null() { base } else { base.wrapping_add(byte_offset) };
        Ok(Some((
            crate::BufferStorage { kind, data, byte_offset, byte_length, length, buffer: root },
            buffer,
        )))
    }

    fn array_buffer_byte_length(&self, buffer: Value) -> usize {
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, detached: false, .. }) => bytes.len(),
            _ => 0,
        }
    }

    /// A DataView's current byte length, zero once detached or out of bounds.
    fn embedding_view_bytes_length(&self, view: Value) -> usize {
        let Some(Cell::DataView { object, buffer, length, length_tracking, .. }) = self.heap.get(view) else {
            return 0;
        };
        if self.array_buffer_detached(*buffer) {
            0
        } else if *length_tracking {
            self.array_buffer_byte_length(*buffer).saturating_sub(object.offset)
        } else if self.array_buffer_out_of_bounds(*buffer, object.offset, *length) {
            0
        } else {
            *length
        }
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
            kind: TypedArrayKind::Uint8,
            object: Box::new(crate::heap::BufferViewObject {
                object: Self::empty_object(self.typed_array_proto(TypedArrayKind::Uint8)),
                offset: 0,
            }),
            buffer: self.heap.root_value(buffer_root).unwrap(),
            length,
            length_tracking: false,
        });
        self.heap.release_root(buffer_root);
        Ok(view)
    }

    pub(crate) fn embedding_view_bytes(&self, value: Value) -> Option<Vec<u8>> {
        self.embedding_view_bytes_prefix(value, usize::MAX)
    }

    pub(crate) fn embedding_bytes_prefix(&self, value: Value, maximum: usize) -> Option<Vec<u8>> {
        if let Some(Cell::ArrayBuffer { bytes, detached, .. }) = self.heap.get(value) {
            let length=bytes.len().min(maximum);return Some(if *detached { Vec::new() } else { bytes.read()[..length].to_vec() });
        }
        self.embedding_view_bytes_prefix(value, maximum)
    }

    /// The backing buffer of an ArrayBuffer or view and the byte range it currently covers.
    fn embedding_buffer_range(&self, value: Value) -> Option<(Value, std::ops::Range<usize>)> {
        let (buffer, offset, byte_length) = match self.heap.get(value)? {
            Cell::ArrayBuffer { bytes, .. } => (value, 0, bytes.len()),
            Cell::TypedArray { object, buffer, kind, .. } => {
                (*buffer, object.offset, self.typed_array_length(value)?.checked_mul(kind.width())?)
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
        let Cell::ArrayBuffer { bytes, detached: false, immutable: false, .. } = self.heap.get_mut(buffer)? else {
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
        let (buffer, offset, byte_length) = match self.heap.get(value)? {
            Cell::TypedArray {
                object,
                buffer,
                kind,
                ..
            } => (
                *buffer,
                object.offset,
                self.typed_array_length(value)?.checked_mul(kind.width())?,
            ),
            Cell::DataView { object, buffer, .. } => {
                (*buffer, object.offset, self.embedding_view_bytes_length(value))
            }
            _ => return None,
        };
        let Cell::ArrayBuffer {
            bytes, detached, ..
        } = self.heap.get(buffer)?
        else {
            return None;
        };
        if *detached {
            return Some(f(&[]));
        }
        let end = offset.checked_add(byte_length)?;
        let bytes = bytes.read();
        Some(f(bytes.get(offset..end)?))
    }

    pub(super) fn typed_array_get(&mut self, object: Value, index: usize) -> Option<Value> {
        let view = self.typed_array_view(object)?;
        let kind = view.kind;
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(view.buffer) else {
            return Some(Value::UNDEFINED);
        };
        // The element's bytes, copied out so a BigInt result can be allocated.
        let mut element = [0; size_of::<u64>()];
        {
            let bytes = bytes.read();
            let Some(start) = view.element(index, bytes.len()) else {
                return Some(Value::UNDEFINED);
            };
            element[..kind.width()].copy_from_slice(&bytes[start..start + kind.width()]);
        }
        let bytes = &element;
        Some(match kind {
            TypedArrayKind::BigInt64 => self.heap.alloc(Cell::bigint(
                i64::from_ne_bytes(bytes[..8].try_into().unwrap()).into(),
            )),
            TypedArrayKind::BigUint64 => self.heap.alloc(Cell::bigint(
                u64::from_ne_bytes(bytes[..8].try_into().unwrap()).into(),
            )),
            _ => Value::number(Self::typed_array_number_of(kind, bytes)),
        })
    }

    /// The Number a numeric typed array element of `kind` holds in its leading `bytes`.
    fn typed_array_number_of(kind: TypedArrayKind, bytes: &[u8; size_of::<u64>()]) -> f64 {
        match kind {
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => bytes[0] as f64,
            TypedArrayKind::Uint16 => u16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
            TypedArrayKind::Uint32 => {
                u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            TypedArrayKind::Int8 => bytes[0] as i8 as f64,
            TypedArrayKind::Int16 => i16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
            TypedArrayKind::Int32 => {
                i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            TypedArrayKind::Float16 => {
                super::number::half_to_f64(u16::from_ne_bytes([bytes[0], bytes[1]]))
            }
            TypedArrayKind::Float32 => {
                f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            TypedArrayKind::Float64 => f64::from_ne_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => unreachable!("a BigInt element is no Number"),
        }
    }

    pub(super) fn typed_array_length(&self, object: Value) -> Option<usize> {
        let (buffer, offset, length, tracking, kind) = match self.heap.get(object) {
            Some(Cell::TypedArray {
                buffer,
                object,
                length,
                length_tracking,
                kind,
                ..
            }) => (*buffer, object.offset, *length, *length_tracking, *kind),
            _ => return None,
        };
        if self.array_buffer_detached(buffer) {
            return Some(0);
        }
        if tracking {
            return Some(match self.heap.get(buffer) {
                Some(Cell::ArrayBuffer { bytes, .. }) => {
                    bytes.len().saturating_sub(offset) / kind.width()
                }
                _ => 0,
            });
        }
        Some(
            if self.array_buffer_out_of_bounds(buffer, offset, length * kind.width()) {
                0
            } else {
                length
            },
        )
    }

    pub(super) fn typed_array_length_is_variable(&self, object: Value) -> bool {
        let Some(Cell::TypedArray {
            buffer,
            length_tracking,
            ..
        }) = self.heap.get(object)
        else {
            return false;
        };
        matches!(
            self.heap.get(*buffer),
            Some(Cell::ArrayBuffer {
                shared,
                resizable: true,
                ..
            }) if !shared || *length_tracking
        )
    }

    pub(super) fn typed_array_out_of_bounds(&self, object: Value) -> bool {
        let Some(Cell::TypedArray {
            buffer,
            object,
            kind,
            length,
            length_tracking,
            ..
        }) = self.heap.get(object)
        else {
            return false;
        };
        let detached = self.array_buffer_detached(*buffer);
        let out = if *length_tracking {
            match self.heap.get(*buffer) {
                Some(Cell::ArrayBuffer { bytes, .. }) => detached || object.offset > bytes.len(),
                _ => true,
            }
        } else {
            self.array_buffer_out_of_bounds(
                *buffer,
                object.offset,
                length.saturating_mul(kind.width()),
            )
        };
        out
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

    pub(super) fn typed_array_kind(&self, object: Value) -> Option<TypedArrayKind> {
        match self.heap.get(object) {
            Some(Cell::TypedArray { kind, .. }) => Some(*kind),
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
    pub(super) fn typed_array_set_number(&mut self, object: Value, index: usize, value: Value) -> bool {
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
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get_mut(view.buffer) else {
            return;
        };
        // An owned store no other cell shares is written in place (`BufferBytes::lane_mut`);
        // any other is unshared or locked first.
        if let Some(store) = bytes.lane_mut() {
            if let Some(start) = view.element(index, store.len()) {
                store_element(store, start, view.kind.width(), &encoded);
            }
            return;
        }
        if let Some(start) = view.element(index, bytes.len()) {
            store_element(&mut bytes.make_mut(), start, view.kind.width(), &encoded);
        }
    }

    /// What an element access reads of the typed array `object` before its buffer: one heap
    /// read, where `typed_array_length` and the buffer checks would each make their own.
    fn typed_array_view(&self, object: Value) -> Option<TypedArrayView> {
        match self.heap.get(object) {
            Some(Cell::TypedArray { kind, object, buffer, length, length_tracking }) => Some(TypedArrayView {
                kind: *kind,
                buffer: *buffer,
                offset: object.offset,
                length: *length,
                length_tracking: *length_tracking,
            }),
            _ => None,
        }
    }

    /// One element encoding serves indexed stores and validated bulk fills.
    fn typed_array_element_bytes(
        kind: TypedArrayKind,
        value: &TypedArrayElement,
    ) -> [u8; MAX_TYPED_ARRAY_ELEMENT_WIDTH] {
        let mut bytes = [0; MAX_TYPED_ARRAY_ELEMENT_WIDTH];
        let number = match value {
            TypedArrayElement::Number(value) => *value,
            _ => 0.0,
        };
        match kind {
            TypedArrayKind::Uint8 | TypedArrayKind::Int8 => {
                bytes[0] = Self::uint8_from_value(number)
            }
            TypedArrayKind::Uint8Clamped => bytes[0] = Self::uint8_clamped_from_value(number),
            TypedArrayKind::Uint16 | TypedArrayKind::Int16 => bytes[..kind.width()]
                .copy_from_slice(&Self::uint16_from_value(number).to_ne_bytes()),
            TypedArrayKind::Uint32 | TypedArrayKind::Int32 => bytes[..kind.width()]
                .copy_from_slice(&Self::uint32_from_value(number).to_ne_bytes()),
            TypedArrayKind::Float16 => bytes[..kind.width()]
                .copy_from_slice(&super::number::f64_to_half(number).to_ne_bytes()),
            TypedArrayKind::Float32 => {
                bytes[..kind.width()].copy_from_slice(&(number as f32).to_ne_bytes())
            }
            TypedArrayKind::Float64 => bytes.copy_from_slice(&number.to_ne_bytes()),
            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                let TypedArrayElement::BigInt(value) = value else {
                    unreachable!("BigInt content type was validated");
                };
                bytes.fill(if value.sign() == num_bigint::Sign::Minus {
                    u8::MAX
                } else {
                    0
                });
                for (target, source) in bytes.iter_mut().zip(value.to_signed_bytes_le()) {
                    *target = source;
                }
            }
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
        let (buffer, offset, kind) = match self.heap.get(object) {
            Some(Cell::TypedArray {
                object: view,
                buffer,
                kind,
                ..
            }) => (*buffer, view.offset, *kind),
            _ => return false,
        };
        if self.array_buffer_detached(buffer)
            || self
                .typed_array_length(object)
                .is_none_or(|length| end > length)
            || self.array_buffer_out_of_bounds(buffer, offset, kind.width() * end)
        {
            return false;
        }
        let encoded = Self::typed_array_element_bytes(kind, value);
        let Some(Cell::ArrayBuffer {
            bytes,
            shared: false,
            ..
        }) = self.heap.get_mut(buffer)
        else {
            return false;
        };
        let bytes =
            &mut bytes.make_mut()[offset + start * kind.width()..offset + end * kind.width()];
        let element = &encoded[..kind.width()];
        if element.iter().all(|byte| *byte == element[0]) {
            bytes.fill(element[0]);
        } else {
            for chunk in bytes.chunks_exact_mut(kind.width()) {
                chunk.copy_from_slice(element);
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
        let Some(Cell::TypedArray {
            kind,
            buffer,
            object,
            ..
        }) = self.heap.get(source)
        else {
            unreachable!("typed copy source was validated");
        };
        let (source_kind, source_buffer, source_offset) = (*kind, *buffer, object.offset);
        let Some(Cell::TypedArray {
            kind,
            buffer,
            object,
            ..
        }) = self.heap.get(target)
        else {
            unreachable!("typed copy target was validated");
        };
        let (target_kind, target_buffer, target_offset) = (*kind, *buffer, object.offset);
        if source_kind.is_bigint() != target_kind.is_bigint() {
            return Err(self.type_error(p, super::operations::MIXED_BIGINT_TYPES.into()));
        }
        if source_kind == target_kind {
            let target_offset = target_offset + target_start * target_kind.width();
            let byte_length = length * source_kind.width();
            if self.array_buffers_share_data_block(source_buffer,target_buffer) {
                let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(target_buffer) else {
                    unreachable!("typed copy backing buffer was validated");
                };
                // Set snapshots overlapping source bytes, unlike slice's forward copy.
                bytes.make_mut()
                    .copy_within(source_offset..source_offset + byte_length, target_offset);
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
    /// bytes: each source element (`typed_array_number_of`) converted as its write converts a
    /// Number (`typed_array_element_bytes`), a run reading the source's bytes before writing
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
        for (from, to) in source.chunks_exact(source_width).zip(target.chunks_exact_mut(target_width)) {
            let mut element = [0; size_of::<u64>()];
            element[..source_width].copy_from_slice(from);
            let number = Self::typed_array_number_of(source_kind, &element);
            let encoded = Self::typed_array_element_bytes(target_kind, &TypedArrayElement::Number(number));
            to.copy_from_slice(&encoded[..target_width]);
        }
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
                self.to_bigint(p, value).map(TypedArrayElement::BigInt)
            }
            _ => self.to_number(p, value).map(TypedArrayElement::Number),
        }
    }
}

#[cfg(test)]
mod backing_address_tests {
    use super::*;
    #[test]
    fn observed_alignment_accounts_owned_bytes_once_and_preserves_original_store() {
        let mut vm = Vm::new(crate::SystemHost);
        let program = crate::Engine::specialize("0;", "alignment-owner.js").unwrap();
        vm.execute(&program).unwrap();
        let buffer = vm.new_fixed_array_buffer(&program, 4096, false).unwrap();
        let root = vm.heap.root(buffer);
        let accounted = vm.heap.stats().5;
        assert!(accounted >= 4096, "a fresh buffer owns its zeroed bytes");
        let offset = vm.embedding_array_buffer_aligned_offset(root, 64).unwrap();
        assert!(offset < 64);
        assert_eq!(vm.heap.stats().5, accounted);
        let snapshot = vm.heap.get(buffer).unwrap().clone();
        let Some(Cell::ArrayBuffer { bytes, .. }) = vm.heap.get_mut(buffer) else {
            panic!("ArrayBuffer");
        };
        bytes.make_mut()[offset] = 17;
        let Cell::ArrayBuffer { bytes, .. } = snapshot else {
            panic!("ArrayBuffer snapshot");
        };
        assert_eq!(bytes.read()[offset], 0);
        assert_eq!(
            vm.embedding_array_buffer_aligned_offset(root, 64).unwrap(),
            offset
        );
        assert_eq!(vm.heap.stats().5, accounted);
        vm.heap.release_root(root);
    }
}

/// A typed array's view of its buffer, as an element access reads it (`Vm::typed_array_view`).
#[derive(Clone, Copy)]
struct TypedArrayView {
    kind: TypedArrayKind,
    buffer: Value,
    offset: usize,
    length: usize,
    length_tracking: bool,
}

impl TypedArrayView {
    /// The byte offset of element `index` in an attached buffer of `byte_length` bytes, or
    /// `None` past the array's length as `Vm::typed_array_length` derives it: a
    /// length-tracking view covers the buffer past its offset, a fixed one covers none of it
    /// once the buffer shrank below its end.
    fn element(self, index: usize, byte_length: usize) -> Option<usize> {
        let width = self.kind.width();
        let length = if self.length_tracking {
            byte_length.saturating_sub(self.offset) / width
        } else if self.offset.saturating_add(self.length * width) > byte_length {
            0
        } else {
            self.length
        };
        (index < length).then(|| self.offset + index * width)
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
