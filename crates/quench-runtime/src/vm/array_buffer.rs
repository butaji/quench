use super::property_key::PropertyKey;
use super::*;

mod install;

/// The caller owns the diagnostic, while conversion and index bounds stay shared.
pub(super) enum BufferIndexUse {
    ArrayBufferLength,
    TypedArrayByteOffset,
}

impl<H: Host> Vm<H> {
    /// Object identity and shared data-block identity are distinct facts.
    pub(super) fn array_buffers_share_data_block(&self,left:Value,right:Value)->bool{
        if left==right{return true}
        let backing=|v|match self.heap.get(v){Some(Cell::ArrayBuffer{bytes,shared:true,..})=>bytes.shared_backing(),_=>None};
        match(backing(left),backing(right)){(Some(a),Some(b))=>a.shares_data_block(&b),_=>false}
    }

    pub(super) fn array_buffer_detached(&self, buffer: Value) -> bool {
        matches!(
            self.heap.get(buffer),
            Some(Cell::ArrayBuffer { detached: true, .. })
        )
    }

    pub(super) fn array_buffer_out_of_bounds(
        &self,
        buffer: Value,
        offset: usize,
        length: usize,
    ) -> bool {
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer {
                bytes, detached, ..
            }) => *detached || offset.saturating_add(length) > bytes.len(),
            _ => true,
        }
    }

    pub(super) fn array_buffer_resizable(&self, buffer: Value) -> bool {
        matches!(
            self.heap.get(buffer),
            Some(Cell::ArrayBuffer {
                resizable: true,
                ..
            })
        )
    }

    pub(super) fn array_buffer_getter_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::ArrayBuffer {
            bytes,
            shared,
            detached,
            max_byte_length,
            resizable,
            immutable,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(
                p,
                "ArrayBuffer accessor called on incompatible receiver".into(),
            ));
        };
        let result = match native {
            Native::ArrayBufferByteLengthGetter if !*shared => {
                Value::number(if *detached { 0.0 } else { bytes.len() as f64 })
            }
            Native::SharedArrayBufferByteLengthGetter if *shared => {
                Value::number(bytes.len() as f64)
            }
            Native::ArrayBufferDetachedGetter if !*shared => {
                if *detached {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::ArrayBufferImmutableGetter if !*shared => {
                if *immutable {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::ArrayBufferMaxByteLengthGetter if !*shared => Value::number(if *detached {
                0.0
            } else if *resizable {
                *max_byte_length as f64
            } else {
                bytes.len() as f64
            }),
            Native::SharedArrayBufferMaxByteLengthGetter if *shared => {
                Value::number(if *resizable {
                    *max_byte_length as f64
                } else {
                    bytes.len() as f64
                })
            }
            Native::ArrayBufferResizableGetter if !*shared => {
                if *resizable {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::SharedArrayBufferGrowableGetter if *shared => {
                if *resizable && !*detached {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => {
                return Err(self.type_error(
                    p,
                    "ArrayBuffer accessor called on incompatible receiver".into(),
                ));
            }
        };
        Ok(result)
    }

    pub(super) fn construct_buffer_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let shared = native == Native::SharedArrayBuffer;
        let length =
            self.array_buffer_to_index(p, args.first().copied().unwrap_or_else(|| Value::number(0.0)))?;
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let max_byte_length_option =
            if options.is_undefined() || self.object_data(options).is_none() {
                None
            } else {
                let atom = self.intern_atom("maxByteLength");
                let value = self.get_property(p, options, atom)?;
                (!value.is_undefined())
                    .then(|| self.array_buffer_to_index(p, value))
                    .transpose()?
            };
        let resizable = max_byte_length_option.is_some();
        let max_byte_length = max_byte_length_option.unwrap_or(length);
        if max_byte_length < length {
            return Err(self.range_error(p, "maxByteLength is less than byteLength".into()));
        }
        if resizable {
            let mut reservation = Vec::<u8>::new();
            reservation
                .try_reserve_exact(max_byte_length)
                .map_err(|_| {
                    self.range_error(p, "ArrayBuffer maxByteLength is too large".into())
                })?;
        }
        let prototype = self.array_buffer_prototype_from_new_target(p, new_target, shared)?;
        let bytes = if shared {
            crate::heap::BufferBytes::from_shared(
                crate::shared_backing::SharedBacking::new(length, max_byte_length, resizable)
                    .map_err(|_| {
                        self.range_error(p, "SharedArrayBuffer allocation failed".into())
                    })?,
            )
        } else {
            self.array_buffer_zeroed_bytes(p, length)?.into()
        };
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Self::empty_object(prototype)),
            bytes,
            shared,
            detached: false,
            max_byte_length,
            resizable,
            immutable: false,
        });
        self.intern_atom("byteLength");
        Ok(buffer)
    }

    fn array_buffer_prototype_from_new_target(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
        shared: bool,
    ) -> Result<Value, JsError> {
        let prototype = self.constructor_prototype_value(p, new_target)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = self.function_realm(p, new_target)?;
        let (native, fallback) = if shared {
            (Native::SharedArrayBuffer, self.shared_array_buffer_proto)
        } else {
            (Native::ArrayBuffer, self.array_buffer_proto)
        };
        Ok(self
            .realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, native))
            .copied()
            .unwrap_or(fallback))
    }

    pub(super) fn array_buffer_zeroed_bytes(
        &mut self,
        p: &ResidualProgram,
        length: usize,
    ) -> Result<Vec<u8>, JsError> {
        zeroed_byte_vec(length)
            .ok_or_else(|| self.range_error(p, "ArrayBuffer length is too large".into()))
    }

    fn array_buffer_intrinsic_prototype(&self, shared: bool) -> Value {
        let native = if shared {
            Native::SharedArrayBuffer
        } else {
            Native::ArrayBuffer
        };
        self.realm.intrinsics.builtin_prototypes[&(self.realm.globals, native)]
    }

    pub(super) fn new_fixed_array_buffer(
        &mut self,
        p: &ResidualProgram,
        length: usize,
        shared: bool,
    ) -> Result<Value, JsError> {
        let bytes = if shared {crate::heap::BufferBytes::from_shared(crate::shared_backing::SharedBacking::new(length,length,false).map_err(|_|self.range_error(p,"SharedArrayBuffer allocation failed".into()))?)}else{self.array_buffer_zeroed_bytes(p,length)?.into()};
        Ok(self.fixed_array_buffer_from(bytes, shared))
    }

    /// A fixed-length intrinsic ArrayBuffer that takes ownership of `bytes`.
    pub(super) fn fixed_array_buffer_from(&mut self, bytes: crate::heap::BufferBytes, shared: bool) -> Value {
        let prototype = self.array_buffer_intrinsic_prototype(shared);
        let length = bytes.len();
        self.heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Self::empty_object(prototype)),
            bytes,
            shared,
            detached: false,
            max_byte_length: length,
            resizable: false,
            immutable: false,
        })
    }

    pub(super) fn array_buffer_slice_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        shared: bool,
    ) -> Result<Value, JsError> {
        let length = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared: receiver_shared,
                detached,
                ..
            }) if *receiver_shared == shared && !detached => bytes.len(),
            _ => {
                let name = if shared {
                    "SharedArrayBuffer"
                } else {
                    "ArrayBuffer"
                };
                return Err(self.type_error(p, format!("{name}.slice receiver is invalid")));
            }
        };
        let start = args
            .first()
            .copied()
            .map(|value| self.array_buffer_slice_index(p, value, length))
            .transpose()?
            .unwrap_or(0);
        let end = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_buffer_slice_index(p, value, length))
            .transpose()?
            .unwrap_or(length);
        let count = end.saturating_sub(start);
        let constructor_atom = self.runtime_atoms.constructor;
        let constructor = self.get_property(p, this, constructor_atom)?;
        let species = if constructor.is_undefined() {
            None
        } else {
            if constructor.is_null() || self.object_data(constructor).is_none() {
                return Err(self.type_error(p, "ArrayBuffer constructor must be an object".into()));
            }
            let species = self.species_of(p, constructor)?;
            if species.is_null() || species.is_undefined() {
                None
            } else {
                Some(species)
            }
        };
        let result = if let Some(species) = species {
            if !self.is_constructable(species) {
                return Err(self.type_error(p, "ArrayBuffer species is not a constructor".into()));
            }
            self.construct_value_with_new_target(
                p,
                species,
                species,
                &[Value::number(count as f64)],
            )?
        } else {
            self.new_fixed_array_buffer(p, count, shared)?
        };
        let (result_bytes, result_shared, result_detached, result_immutable) =
            match self.heap.get(result) {
                Some(Cell::ArrayBuffer {
                    bytes,
                    shared,
                    detached,
                    immutable,
                    ..
                }) => (bytes.clone(), *shared, *detached, *immutable),
                _ => {
                    return Err(
                        self.type_error(p, "ArrayBuffer species must return an ArrayBuffer".into())
                    );
                }
            };
        if self.array_buffers_share_data_block(result, this)
            || result_shared != shared
            || result_detached
            || result_immutable
        {
            return Err(self.type_error(p, "ArrayBuffer species returned an invalid buffer".into()));
        }
        if result_bytes.len() < count {
            return Err(self.type_error(p, "ArrayBuffer species returned a short buffer".into()));
        }
        // Coercions and species construction can detach, resize, or write the
        // source. Its current backing store is authoritative at the copy step.
        let source_bytes = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                detached: false,
                ..
            }) => bytes.to_vec(),
            _ => return Err(self.type_error(p, "ArrayBuffer was detached during slice".into())),
        };
        let copy_count = count.min(source_bytes.len().saturating_sub(start));
        if copy_count != 0
            && let Some(Cell::ArrayBuffer {
                bytes: destination, ..
            }) = self.heap.get_mut(result)
        {
            destination.make_mut()[..copy_count]
                .copy_from_slice(&source_bytes[start..start + copy_count]);
        }
        Ok(result)
    }

    pub(super) fn array_buffer_transfer_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        preserve_resizability: bool,
        immutable_result: bool,
    ) -> Result<Value, JsError> {
        let (bytes, shared, max_byte_length, resizable) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                max_byte_length,
                resizable,
                ..
            }) => (bytes.clone(), *shared, *max_byte_length, *resizable),
            _ => return Err(self.type_error(p, "ArrayBuffer.transfer receiver is invalid".into())),
        };
        if shared {
            return Err(self.type_error(p, "ArrayBuffer transfer requires an ArrayBuffer".into()));
        }
        let length = args
            .first()
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_buffer_to_index(p, value))
            .transpose()?
            .unwrap_or(bytes.len());
        let (detached, immutable, bytes) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                detached,
                immutable,
                ..
            }) => (*detached, *immutable, bytes.clone()),
            _ => (true, true, Default::default()),
        };
        if self
            .object_data(this)
            .is_some_and(Object::is_structured_transfer_forbidden)
        {
            return Err(self.type_error(
                p,
                "ArrayBuffer is not detachable and could not be cloned.".into(),
            ));
        }
        if detached || immutable || bytes.is_wasm_view() {
            return Err(self.type_error(p, "ArrayBuffer is not transferable".into()));
        }
        if preserve_resizability && resizable && length > max_byte_length {
            return Err(self.range_error(p, "ArrayBuffer length exceeds maxByteLength".into()));
        }
        let mut copied_bytes = Vec::new();
        copied_bytes
            .try_reserve_exact(length)
            .map_err(|_| self.range_error(p, "ArrayBuffer length is too large".into()))?;
        copied_bytes.resize(length, 0);
        let copied = bytes.len().min(length);
        copied_bytes[..copied].copy_from_slice(&bytes.read()[..copied]);
        let result_max_byte_length = if preserve_resizability && resizable {
            max_byte_length
        } else {
            length
        };
        let result = self.heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Self::empty_object(
                self.array_buffer_intrinsic_prototype(false),
            )),
            bytes: copied_bytes.into(),
            shared: false,
            detached: false,
            max_byte_length: result_max_byte_length,
            resizable: preserve_resizability && resizable,
            immutable: immutable_result,
        });
        self.detach_array_buffer(this);
        Ok(result)
    }

    /// DetachArrayBuffer once the caller has checked the detach key: releases the bytes.
    pub(super) fn detach_array_buffer(&mut self, buffer: Value) {
        if let Some(Cell::ArrayBuffer {
            bytes, detached, ..
        }) = self.heap.get_mut(buffer)
        {
            let before = std::mem::take(bytes).capacity();
            *detached = true;
            self.heap.adjust_external_bytes(before, 0);
        }
    }

    pub(super) fn array_buffer_to_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<usize, JsError> {
        self.buffer_to_index(p, value, BufferIndexUse::ArrayBufferLength)
    }

    pub(super) fn buffer_to_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        usage: BufferIndexUse,
    ) -> Result<usize, JsError> {
        let number = self.to_number(p, value)?;
        let Some(index) = crate::units::to_index(number) else {
            let message = match usage {
                BufferIndexUse::ArrayBufferLength => {
                    "ArrayBuffer length is out of range".to_owned()
                }
                BufferIndexUse::TypedArrayByteOffset => format!(
                    "Start offset {} is outside the bounds of the buffer",
                    crate::number_to_string::format(number),
                ),
            };
            return Err(self.range_error(p, message));
        };
        Ok(index)
    }

    pub(super) fn array_buffer_resize_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (immutable, detached, shared, resizable, max_byte_length) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                immutable,
                detached,
                shared,
                resizable,
                max_byte_length,
                ..
            }) => (*immutable, *detached, *shared, *resizable, *max_byte_length),
            _ => return Err(self.type_error(p, "ArrayBuffer.resize receiver is invalid".into())),
        };
        if immutable {
            return Err(self.type_error(p, "cannot resize an immutable ArrayBuffer".into()));
        }
        let requested =
            self.array_buffer_to_index(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if shared || detached || !resizable {
            return Err(self.type_error(p, "ArrayBuffer is not resizable".into()));
        }
        if requested > max_byte_length {
            return Err(self.range_error(p, "ArrayBuffer resize exceeds maxByteLength".into()));
        }
        let can_resize = matches!(
            self.heap.get(this),
            Some(Cell::ArrayBuffer {
                shared: false,
                detached: false,
                immutable: false,
                resizable: true,
                max_byte_length,
                ..
            }) if requested <= *max_byte_length
        );
        if !can_resize {
            return Err(self.type_error(p, "ArrayBuffer is not resizable".into()));
        }
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(this) else {
            unreachable!("receiver validated before mutation");
        };
        let previous = bytes.clone();
        let before = previous.capacity();
        let mut resized = self.array_buffer_zeroed_bytes(p, requested)?;
        let copy_length = previous.len().min(requested);
        resized[..copy_length].copy_from_slice(&previous.read()[..copy_length]);
        let after = resized.capacity();
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(this) else {
            unreachable!("receiver validated before mutation");
        };
        *bytes = resized.into();
        self.heap.adjust_external_bytes(before, after);
        Ok(Value::UNDEFINED)
    }

    pub(super) fn shared_array_buffer_grow_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let requested =
            self.array_buffer_to_index(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let (current_length, max_length, shared, detached, resizable) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                max_byte_length,
                resizable,
                ..
            }) => (
                bytes.len(),
                *max_byte_length,
                *shared,
                *detached,
                *resizable,
            ),
            _ => {
                return Err(self.type_error(p, "SharedArrayBuffer.grow receiver is invalid".into()));
            }
        };
        if !shared || detached || !resizable {
            return Err(self.type_error(p, "SharedArrayBuffer is not growable".into()));
        }
        if requested < current_length || requested > max_length {
            return Err(self.range_error(p, "SharedArrayBuffer grow length is out of range".into()));
        }
        if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(this)
            && let Some(backing) = bytes.shared_backing() {
                backing
                    .grow(requested)
                    .map_err(|_| self.range_error(p, "SharedArrayBuffer grow failed".into()))?;
                return Ok(Value::UNDEFINED);
            }
        let (before, after) = {
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(this) else {
                unreachable!("grow receiver validated before mutation");
            };
            let before = bytes.capacity();
            bytes.owned_mut().resize(requested, 0);
            (before, bytes.capacity())
        };
        self.heap.adjust_external_bytes(before, after);
        Ok(Value::UNDEFINED)
    }

    pub(super) fn array_buffer_slice_immutable_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source_length = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared: false,
                detached: false,
                ..
            }) => bytes.len(),
            _ => {
                return Err(self.type_error(
                    p,
                    "sliceToImmutable requires an attached ArrayBuffer".into(),
                ));
            }
        };
        let start = args
            .first()
            .copied()
            .map(|value| self.array_buffer_slice_index(p, value, source_length))
            .transpose()?
            .unwrap_or(0);
        let end = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_buffer_slice_index(p, value, source_length))
            .transpose()?
            .unwrap_or(source_length)
            .max(start);
        let (source_bytes, detached) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes, detached, ..
            }) => (bytes.to_vec(), *detached),
            _ => (Default::default(), true),
        };
        if detached {
            return Err(self.type_error(p, "ArrayBuffer was detached during coercion".into()));
        }
        if source_bytes.len() < end {
            return Err(self.range_error(p, "ArrayBuffer was resized during coercion".into()));
        }
        let count = end - start;
        let mut bytes = self.array_buffer_zeroed_bytes(p, count)?;
        bytes.copy_from_slice(&source_bytes[start..end]);
        Ok(self.heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Self::empty_object(
                self.array_buffer_intrinsic_prototype(false),
            )),
            bytes: bytes.into(),
            shared: false,
            detached: false,
            max_byte_length: count,
            resizable: false,
            immutable: true,
        }))
    }

    fn array_buffer_slice_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        length: usize,
    ) -> Result<usize, JsError> {
        Ok(crate::units::relative_index(self.to_number(p, value)?, length))
    }
}

/// Zeroed bytes straight from the allocator (`calloc`/`mmap` for large sizes), so pages that are
/// never written stay unmapped instead of being touched by a fill.
pub(crate) fn zeroed_byte_vec(length: usize) -> Option<Vec<u8>> {
    if length == 0 {
        return Some(Vec::new());
    }
    let layout = std::alloc::Layout::array::<u8>(length).ok()?;
    // SAFETY: `layout` is non-zero-sized; a non-null result is `length` zeroed bytes allocated
    // with the global allocator and `u8` layout, exactly what `Vec<u8>` owns.
    unsafe {
        let ptr = std::alloc::alloc_zeroed(layout);
        (!ptr.is_null()).then(|| Vec::from_raw_parts(ptr, length, length))
    }
}
