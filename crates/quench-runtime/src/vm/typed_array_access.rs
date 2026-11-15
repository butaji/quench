use super::*;

mod embedding;
mod view_cache;
pub(in crate::vm) use view_cache::TypedViewCache;

const MAX_TYPED_ARRAY_ELEMENT_WIDTH: usize = std::mem::size_of::<u64>();

pub(super) enum TypedArrayElement {
    BigInt(num_bigint::BigInt),
    Number(f64),
}

impl<H: Host> Vm<H> {
    /// `plain_typed_element`'s common case, inline: a canonical integer key inside a fixed
    /// length typed array of Number elements over an owned buffer, of kind `KIND`
    /// (`TypedArrayKind as usize`, a site's one kind) or of any kind (`ANY_TYPED_KIND`).
    /// `None` leaves the read to `plain_typed_element`, which answers every other case
    /// (`undefined` past the end, a tracking or detached view, Float16, a double key).
    #[inline(always)]
    pub(super) fn typed_element_quick<const KIND: usize>(&self, object: Value, key: Value) -> Option<Value> {
        let Some(Cell::TypedArray { object: view, buffer, length }) = self.heap.get(object) else {
            return None;
        };
        let kind = typed_kind::<KIND>(view.native.kind())?;
        let length = length.get()?;
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(*buffer) else {
            return None;
        };
        let store = bytes.owned()?;
        let start = typed_element_start(kind, view.offset, length, key, store.len())?;
        // SAFETY: `typed_element_start` proved the element's bytes inside the store.
        unsafe { read_element(kind, store.as_ptr().add(start)) }
    }

    /// `typed_array_set_number`'s common case, inline: a Number stored at a canonical integer
    /// key inside a fixed length typed array of kind `KIND` (as `typed_element_quick`) whose
    /// buffer the view owns alone (`BufferBytes::lane_mut`), of an integer into an integer
    /// kind or of any Number into a Float32 or Float64 kind. False leaves the store to
    /// `typed_array_set_number`, which stores nothing before it decides.
    #[inline(always)]
    pub(super) fn typed_set_quick<const KIND: usize>(&mut self, object: Value, key: Value, value: Value) -> bool {
        let Some(Cell::TypedArray { object: view, buffer, length }) = self.heap.get(object) else {
            return false;
        };
        let Some(kind) = typed_kind::<KIND>(view.native.kind()) else {
            return false;
        };
        let (offset, buffer) = (view.offset, *buffer);
        let Some(length) = length.get() else {
            return false;
        };
        let Some(Cell::ArrayBuffer { bytes, detached: false, immutable: false, .. }) = self.heap.buffer_bytes_mut(buffer) else {
            return false;
        };
        let Some(store) = bytes.lane_mut() else {
            return false;
        };
        let Some(start) = typed_element_start(kind, offset, length, key, store.len()) else {
            return false;
        };
        // SAFETY: `typed_element_start` proved the element's bytes inside the store.
        unsafe { write_quick_element(kind, store.as_mut_ptr().add(start), value) }
    }

    /// The kind of the typed array `object`, which picks a site's kind variant
    /// (`typed_element_quick`).
    pub(super) fn typed_array_kind_of(&self, object: Value) -> Option<TypedArrayKind> {
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
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(view.buffer) else {
            return Some(Value::UNDEFINED);
        };
        let bytes = bytes.read();
        let Some(start) = view.element(index, bytes.len()) else {
            return Some(Value::UNDEFINED);
        };
        Some(match kind {
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => Value::integer(i32::from(bytes[start])),
            TypedArrayKind::Int8 => Value::integer(i32::from(bytes[start] as i8)),
            TypedArrayKind::Uint16 => Value::integer(i32::from(u16::from_ne_bytes([bytes[start], bytes[start + 1]]))),
            TypedArrayKind::Int16 => Value::integer(i32::from(i16::from_ne_bytes([bytes[start], bytes[start + 1]]))),
            TypedArrayKind::Int32 => Value::integer(i32::from_ne_bytes(
                bytes[start..start + 4].try_into().expect("an Int32 element has four bytes"),
            )),
            _ => {
                let mut element = [0; size_of::<u64>()];
                element[..kind.width()].copy_from_slice(&bytes[start..start + kind.width()]);
                Value::number(Self::typed_array_number_of(kind, &element))
            }
        })
    }

    /// A lane loop's view of the typed array `object`'s elements: its first element's address,
    /// element count and kind, which native code reads (and, `writable`, replaces) in place
    /// until the loop calls out. Only an attached buffer of Number elements this heap holds
    /// qualifies, and to write, one the buffer owns alone (`BufferBytes::lane_mut`), so no
    /// store copies or moves it; anything else stays on the general path.
    pub(super) fn lane_typed_array_elements(&mut self, object: Value, writable: bool) -> Option<(*mut u8, usize, TypedArrayKind)> {
        let view = self.typed_array_view(object)?;
        if view.kind.is_bigint() || view.kind == TypedArrayKind::Float16 {
            return None;
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get_mut(view.buffer) else {
            return None;
        };
        let (base, byte_length) = if writable {
            let store = bytes.lane_mut()?;
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

    /// The slow arm of the staged typed view proof: publishes the elements of the typed array
    /// `object` in its record's native prefix as its buffer is now, which stays true until the
    /// next buffer event (`buffer_relocations`); a view native code cannot address is withdrawn
    /// instead. Whether it is addressable.
    pub(super) fn publish_typed_view(&mut self, object: Value) -> bool {
        if matches!(self.heap.get(object), Some(Cell::TypedArray { object, .. }) if object.native.is_retracted()) {
            return false;
        }
        let unshared = self.lane_typed_array_elements(object, true);
        let published = unshared.or_else(|| self.lane_typed_array_elements(object, false))
            .and_then(|(data, length, _)| Some((data, u32::try_from(i32::try_from(length).ok()?).ok()?)));
        let Some(Cell::TypedArray { object: record, .. }) = self.heap.get_mut(object) else { return false };
        match published {
            Some((data, length)) => record.native.publish(data, length, unshared.is_some()),
            None => record.native.retract(),
        }
        published.is_some()
    }

    pub(super) fn typed_array_get(&mut self, object: Value, index: usize) -> Option<Value> {
        let view = self.typed_array_view(object)?;
        let kind = view.kind;
        if let Some(number) = self.staged_typed_number(object, kind, index) {
            return Some(Value::number(number));
        }
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

    pub(super) fn typed_array_kind(&self, object: Value) -> Option<TypedArrayKind> {
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
    pub(super) fn typed_array_set_number(&mut self, object: Value, index: usize, value: Value) -> bool {
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
    /// view owns alone (`BufferBytes::lane_mut`): an integer's ToInt8 through ToUint32 is its low
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
        let Some(store) = store.lane_mut() else { return false };
        // A fixed view of a fixed buffer is in bounds by its length; past it nothing is written.
        let start = match view.length.get() {
            Some(length) if !*resizable => (index < length).then(|| view.offset + index * width),
            _ => view.element(index, store.len()),
        };
        if let Some(start) = start {
            match view.kind {
                TypedArrayKind::Uint8Clamped => store[start] = integer.clamp(0, i32::from(u8::MAX)) as u8,
                TypedArrayKind::Uint8 | TypedArrayKind::Int8 => store[start] = integer as u8,
                TypedArrayKind::Uint16 | TypedArrayKind::Int16 => store[start..start + size_of::<u16>()].copy_from_slice(&(integer as u16).to_ne_bytes()),
                _ => store[start..start + size_of::<u32>()].copy_from_slice(&(integer as u32).to_ne_bytes()),
            }
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

    /// Whether `value` is a typed array, whose elements a loop region reads in place
    /// (`lane_typed_array_elements`) and a function body has no row for.
    pub(super) fn is_typed_array(&self, value: Value) -> bool {
        self.typed_array_view(value).is_some()
    }

    /// What an element access reads of the typed array `object` before its buffer: one heap
    /// read, where `typed_array_length` and the buffer checks would each make their own.
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
                ..
            }) => (*buffer, view.offset, view.native.kind()),
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
                self.to_bigint(p, value).map(TypedArrayElement::BigInt)
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
pub(super) const ANY_TYPED_KIND: usize = usize::MAX;

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

/// The byte offset of the element at `key` of a fixed view of `length` elements of `kind`
/// from `offset` in a store of `store_length` bytes, where the view lies inside the store
/// (`ViewLength::within`) and the key is an int32 inside the view: one unsigned compare of
/// the key's offset (`Value::int_key_offset`) against a bound below 2^31 tests its tag, sign
/// and bound. `None` otherwise.
#[inline(always)]
fn typed_element_start(kind: TypedArrayKind, offset: usize, length: usize, key: Value, store_length: usize) -> Option<usize> {
    let width = kind.width();
    let index = key.int_key_offset();
    (index < length.min(i32::MAX as usize) && offset.saturating_add(length.saturating_mul(width)) <= store_length)
        .then(|| offset + index * width)
}

/// Reads the element of `kind` at `at` as the Value a quick typed array read answers; `None`
/// for a kind no quick read takes.
///
/// # Safety
/// `at` is valid for a read of `kind`'s width.
#[inline(always)]
unsafe fn read_element(kind: TypedArrayKind, at: *const u8) -> Option<Value> {
    macro_rules! element {
        ($type:ty) => {
            // SAFETY: the caller's `at` is valid for `kind`'s width.
            unsafe { at.cast::<$type>().read_unaligned() }
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

/// Writes `value` as an element of `kind` at `target`, when it stores exactly as an
/// integer into an integer kind or a Number into a float kind. False, with nothing written,
/// for any other value.
///
/// # Safety
/// `target` is valid for a write of `kind`'s width.
#[inline(always)]
unsafe fn write_quick_element(kind: TypedArrayKind, target: *mut u8, value: Value) -> bool {
    macro_rules! store {
        ($element:expr) => {{
            let element = $element;
            // SAFETY: the caller's `target` is valid for `kind`'s width.
            unsafe { write_element(target, element) };
            true
        }};
    }
    match (kind, value.as_int()) {
        (TypedArrayKind::Uint8 | TypedArrayKind::Int8, Some(integer)) => store!(integer as u8),
        (TypedArrayKind::Uint8Clamped, Some(integer)) => store!(integer.clamp(0, i32::from(u8::MAX)) as u8),
        (TypedArrayKind::Uint16 | TypedArrayKind::Int16, Some(integer)) => store!(integer as u16),
        (TypedArrayKind::Uint32 | TypedArrayKind::Int32, Some(integer)) => store!(integer as u32),
        (TypedArrayKind::Float32, _) => value.as_number().is_some_and(|number| store!(number as f32)),
        (TypedArrayKind::Float64, _) => value.as_number().is_some_and(|number| store!(number)),
        _ => false,
    }
}

/// The first element of a view of `length` elements of `width` bytes at `offset` in its
/// buffer's store, and whether the view may be written: a store the buffer owns alone and
/// that is mutable. `None` when the view lies outside the store, or the store is shared.
fn view_pointer(bytes: &mut crate::heap::BufferBytes, offset: usize, length: usize, width: usize, immutable: bool) -> Option<(*mut u8, bool)> {
    let end = offset.saturating_add(length.saturating_mul(width));
    match bytes.lane_mut() {
        // SAFETY: the view lies inside the store (checked just above).
        Some(store) if end <= store.len() => Some((unsafe { store.as_mut_ptr().add(offset) }, !immutable)),
        Some(_) => None,
        None => match bytes.owned() {
            // SAFETY: as above; read only (not writable).
            Some(store) if end <= store.len() => Some((unsafe { store.as_ptr().add(offset) }.cast_mut(), false)),
            _ => None,
        },
    }
}

/// Writes `element` at `target`, which need not be aligned.
///
/// # Safety
/// `target` is valid for a write of `size_of::<T>()` bytes.
#[inline(always)]
unsafe fn write_element<T>(target: *mut u8, element: T) {
    // SAFETY: the caller's contract.
    unsafe { target.cast::<T>().write_unaligned(element) }
}

/// A loop kernel's view of a typed array's elements (`Vm::typed_array_span`).
#[derive(Clone, Copy)]
pub(crate) struct TypedSpan {
    /// The first element's byte.
    pub(crate) first: *mut u8,
    pub(crate) kind: TypedArrayKind,
    pub(crate) length: usize,
    /// Whether a store may write the elements in place: the buffer's bytes are this
    /// buffer's alone and mutable.
    pub(crate) writable: bool,
    /// The buffer's bytes, which two views over one buffer share.
    pub(crate) store: (usize, usize),
}

impl<H: Host> Vm<H> {
    /// The elements of the fixed-length typed array `object` of a Number kind, as a loop
    /// kernel reads and writes them without guest code; `None` for any other value, a
    /// detached, length-tracking, shared or Wasm buffer, or a BigInt or Float16 kind. Valid
    /// while nothing allocates, detaches or resizes.
    pub(crate) fn typed_array_span(&mut self, object: Value) -> Option<TypedSpan> {
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
        let (store, writable) = match bytes.lane_mut().filter(|_| !immutable) {
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
