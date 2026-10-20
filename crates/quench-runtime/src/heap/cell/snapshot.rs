//! Snapshot encoding of cells (`crate::snapshot`): each cell's Rust structure, field by field.
//! Values are written as their bits, every heap index being restored where it was.

use super::*;
use crate::snapshot::{read_list, snapshot_enum, Reader, SnapshotError, SnapshotResult, Writer};

/// What decoding a cell needs from the VM it is restored into.
pub(crate) struct CellDecoder<'a> {
    /// The matcher of a regular expression, compiled again from its source and flags.
    pub(crate) regexp: &'a mut dyn FnMut(&JsString, &str) -> SnapshotResult<Rc<quench_regexp::Regex>>,
}

snapshot_enum!(proxy_kind: ProxyKind { Object, Callable, Constructor });
snapshot_enum!(typed_array_kind: TypedArrayKind {
    Uint8, Uint8Clamped, Uint16, Uint32, Int8, Int16, Int32, BigInt64, BigUint64, Float16, Float32, Float64,
});
snapshot_enum!(iterator_kind: IteratorKind {
    Array, ArrayKeys, ArrayValues, ArrayEntries, String, MapKeys, MapValues, MapEntries, SetValues,
    SetEntries, Protocol, RegExpStringMatchAll, IntlSegments, Map, Filter, Take, Drop, FlatMap, Concat,
    Zip, Generator, AsyncFromSync, AsyncGenerator,
});
snapshot_enum!(prototype_lookup: PrototypeLookupState { Ordinary, Setup, Ready });
snapshot_enum!(structured_serialization: StructuredSerialization { Ordinary, Forbidden, TransferRequired });
snapshot_enum!(symbol_kind: SymbolKind { Public, HostPrivate });
snapshot_enum!(promise_state: crate::heap::PromiseState { Pending, Fulfilled, Rejected });
snapshot_enum!(promise_rejection: crate::heap::PromiseRejectionState {
    Unobserved, Observed, AwaitingReport, Reporting, Reported, HandledReported, Handled,
});
snapshot_enum!(lexical_binding_kind: crate::bytecode::LexicalBindingKind { Mutable, Immutable, FunctionName, PrivateName });

/// The cell tags, in the order `Cell` declares its variants.
mod tag {
    pub(super) const OBJECT: u8 = 0;
    pub(super) const ARRAY: u8 = 1;
    pub(super) const ARRAY_BUFFER: u8 = 2;
    pub(super) const TYPED_ARRAY: u8 = 3;
    pub(super) const DATA_VIEW: u8 = 4;
    pub(super) const MAP: u8 = 5;
    pub(super) const SET: u8 = 6;
    pub(super) const SHADOW_REALM: u8 = 7;
    pub(super) const WEAK_MAP: u8 = 8;
    pub(super) const WEAK_SET: u8 = 9;
    pub(super) const WEAK_REF: u8 = 10;
    pub(super) const FINALIZATION_REGISTRY: u8 = 11;
    pub(super) const ITERATOR: u8 = 12;
    pub(super) const PROXY: u8 = 13;
    pub(super) const FUNCTION: u8 = 14;
    pub(super) const BINDING_REFERENCE: u8 = 15;
    pub(super) const ENVIRONMENT: u8 = 16;
    pub(super) const STRING: u8 = 17;
    pub(super) const BIGINT: u8 = 18;
    pub(super) const SYMBOL: u8 = 19;
    pub(super) const DATE: u8 = 20;
    pub(super) const REGEXP: u8 = 21;
    pub(super) const ERROR: u8 = 22;
    pub(super) const PROMISE: u8 = 23;
    pub(super) const PROMISE_RESOLVING_STATE: u8 = 24;
}

impl Native {
    /// One past the last native's discriminant.
    const COUNT: u16 = Native::CallSiteGetFunction as u16 + 1;

    pub(crate) fn write_snapshot(self, writer: &mut Writer) {
        writer.u16(self as u16);
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let index = reader.u16()?;
        if index >= Self::COUNT {
            return Err(SnapshotError("invalid snapshot native".into()));
        }
        // SAFETY: `Native` is a fieldless `repr(u16)` enum whose discriminants run from zero
        // to `COUNT - 1` without gaps, as it declares no explicit discriminant.
        Ok(unsafe { std::mem::transmute::<u16, Native>(index) })
    }
}

impl FunctionKind {
    pub(crate) fn write_snapshot(self, writer: &mut Writer) {
        match self {
            Self::User(program, function) => {
                writer.u8(0);
                writer.u32(program.raw());
                writer.u32(function);
            }
            Self::NumericUser(program, function) => {
                writer.u8(1);
                writer.u32(program.raw());
                writer.u32(function);
            }
            Self::Native(native) => {
                writer.u8(2);
                native.write_snapshot(writer);
            }
        }
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(match reader.u8()? {
            0 => Self::User(ProgramId::from_raw(reader.u32()?), reader.u32()?),
            1 => Self::NumericUser(ProgramId::from_raw(reader.u32()?), reader.u32()?),
            2 => Self::Native(Native::read_snapshot(reader)?),
            _ => return Err(SnapshotError("invalid snapshot function kind".into())),
        })
    }
}

impl JsString {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) {
        match self.units() {
            crate::vm::wtf16::Units::Ascii(bytes) => {
                writer.u8(0);
                writer.bytes(bytes);
            }
            crate::vm::wtf16::Units::Wide(units) => {
                writer.u8(1);
                writer.u16s(units);
            }
        }
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(match reader.u8()? {
            0 => Self::from_unit_run(crate::vm::wtf16::Units::Ascii(reader.bytes()?)),
            1 => Self::from_units(&reader.u16s()?),
            _ => return Err(SnapshotError("invalid snapshot string".into())),
        })
    }
}

fn write_object(writer: &mut Writer, object: &Object) -> SnapshotResult<()> {
    writer.u32(object.meta);
    writer.u32(object.auxiliary);
    match object.extras() {
        None => {
            // SAFETY: an untagged payload is plain words; an array's second word is its element
            // backing, which the array writes instead.
            let words = unsafe { object.payload.words };
            writer.u8(0);
            writer.u64(words[0]);
            writer.u64(if object.is_array() { 0 } else { words[1] });
        }
        Some(extras) => {
            // SAFETY: as above; the first word is the extras pointer, written field by field.
            let words = unsafe { object.payload.words };
            writer.u8(1);
            writer.u64(if object.is_array() { 0 } else { words[1] });
            write_extras(writer, extras)?;
        }
    }
    Ok(())
}

fn read_object(reader: &mut Reader<'_>) -> SnapshotResult<Object> {
    let meta = reader.u32()?;
    let auxiliary = reader.u32()?;
    let words = match reader.u8()? {
        0 => [reader.u64()?, reader.u64()?],
        1 => {
            let second = reader.u64()?;
            let extras = read_extras(reader)?;
            [Object::tag_extras(Box::new(extras)), second]
        }
        _ => return Err(SnapshotError("invalid snapshot object".into())),
    };
    const _: () = assert!(INLINE_PROPERTY_COUNT == 2);
    Ok(Object { meta, auxiliary, payload: ObjectPayload { words } })
}

fn write_extras(writer: &mut Writer, extras: &ObjectExtras) -> SnapshotResult<()> {
    let ObjectExtras {
        prototype_lookup: lookup,
        inline_properties,
        arena_start,
        arguments_map,
        arguments_object,
        raw_json,
        error_data,
        module_namespace,
        external_address,
        boxed_primitive,
        structured_serialization: serialization,
        structured_transfer_forbidden,
        module_bindings,
        deferred_module,
        private_elements,
        stack_data,
        indexed_descriptors,
        length_descriptor,
    } = extras;
    if external_address.is_some() {
        return Err(SnapshotError::unsupported("an object whose address an embedder observed"));
    }
    prototype_lookup::write(writer, *lookup);
    writer.values(&inline_properties.0);
    writer.u32(*arena_start);
    writer.bool(arguments_map.is_some());
    if let Some(map) = arguments_map {
        writer.u16s(map);
    }
    for flag in [*arguments_object, *raw_json, *error_data, *module_namespace, *structured_transfer_forbidden, *indexed_descriptors, *length_descriptor] {
        writer.bool(flag);
    }
    writer.option_value(*boxed_primitive);
    structured_serialization::write(writer, *serialization);
    writer.len(module_bindings.len());
    for (atom, program, slot) in module_bindings {
        writer.u32(*atom);
        writer.u32(program.raw());
        writer.u16(*slot);
    }
    writer.bool(deferred_module.is_some());
    if let Some(module) = deferred_module {
        write_module_source(writer, module);
    }
    writer.len(private_elements.len());
    for element in private_elements {
        writer.value(element.brand.home);
        writer.u32(element.brand.name);
        writer.option_value(element.data);
    }
    writer.bool(stack_data.is_some());
    if let Some(stack) = stack_data {
        write_stack_data(writer, stack)?;
    }
    Ok(())
}

fn read_extras(reader: &mut Reader<'_>) -> SnapshotResult<ObjectExtras> {
    let prototype_lookup = prototype_lookup::read(reader)?;
    let inline = reader.values()?;
    let inline_properties = DisplacedInline(
        inline.try_into().map_err(|_| SnapshotError("invalid snapshot inline properties".into()))?,
    );
    let arena_start = reader.u32()?;
    let arguments_map = if reader.bool()? { Some(reader.u16s()?) } else { None };
    let [arguments_object, raw_json, error_data, module_namespace, structured_transfer_forbidden, indexed_descriptors, length_descriptor] =
        [reader.bool()?, reader.bool()?, reader.bool()?, reader.bool()?, reader.bool()?, reader.bool()?, reader.bool()?];
    let boxed_primitive = reader.option_value()?;
    let structured_serialization = structured_serialization::read(reader)?;
    let module_bindings = read_list(reader, |reader| Ok((reader.u32()?, ProgramId::from_raw(reader.u32()?), reader.u16()?)))?;
    let deferred_module = if reader.bool()? { Some(read_module_source(reader)?) } else { None };
    let private_elements = read_list(reader, |reader| {
        Ok(PrivateElement {
            brand: PrivateBrand { home: reader.value()?, name: reader.u32()? },
            data: reader.option_value()?,
        })
    })?;
    let stack_data = if reader.bool()? { Some(read_stack_data(reader)?) } else { None };
    Ok(ObjectExtras {
        prototype_lookup,
        inline_properties,
        arena_start,
        arguments_map,
        arguments_object,
        raw_json,
        error_data,
        module_namespace,
        external_address: None,
        boxed_primitive,
        structured_serialization,
        structured_transfer_forbidden,
        module_bindings,
        deferred_module,
        private_elements,
        stack_data,
        indexed_descriptors,
        length_descriptor,
    })
}

fn write_module_source(writer: &mut Writer, module: &crate::ModuleSource) {
    writer.str(&module.name);
    writer.str(&module.source);
    writer.bool(module.original_bytes.is_some());
    if let Some(bytes) = &module.original_bytes {
        writer.bytes(bytes);
    }
}

fn read_module_source(reader: &mut Reader<'_>) -> SnapshotResult<crate::ModuleSource> {
    Ok(crate::ModuleSource {
        name: reader.string()?,
        source: reader.string()?,
        original_bytes: if reader.bool()? { Some(reader.bytes()?.to_vec()) } else { None },
    })
}

fn write_call_site(writer: &mut Writer, record: &CallSiteRecord) {
    let CallSiteRecord { location, name, this_value, hide_identity, is_constructor, eval_origin } = record;
    match location {
        CallSiteLocation::Source { file_name, line, column } => {
            writer.u8(0);
            writer.str(file_name);
            writer.u32(*line);
            writer.u32(*column);
        }
        CallSiteLocation::Native => writer.u8(1),
    }
    match name {
        CallSiteName::Callable(value) => {
            writer.u8(0);
            writer.value(*value);
        }
        CallSiteName::Static(text) => {
            writer.u8(1);
            text.write_snapshot(writer);
        }
        CallSiteName::Anonymous => writer.u8(2),
    }
    writer.value(*this_value);
    writer.bool(*hide_identity);
    writer.bool(*is_constructor);
    writer.bool(eval_origin.is_some());
    if let Some(origin) = eval_origin {
        origin.write_snapshot(writer);
    }
}

fn read_call_site(reader: &mut Reader<'_>) -> SnapshotResult<CallSiteRecord> {
    let location = match reader.u8()? {
        0 => CallSiteLocation::Source { file_name: reader.string()?, line: reader.u32()?, column: reader.u32()? },
        1 => CallSiteLocation::Native,
        _ => return Err(SnapshotError("invalid snapshot call site".into())),
    };
    let name = match reader.u8()? {
        0 => CallSiteName::Callable(reader.value()?),
        1 => CallSiteName::Static(JsString::read_snapshot(reader)?),
        2 => CallSiteName::Anonymous,
        _ => return Err(SnapshotError("invalid snapshot call site name".into())),
    };
    Ok(CallSiteRecord {
        location,
        name,
        this_value: reader.value()?,
        hide_identity: reader.bool()?,
        is_constructor: reader.bool()?,
        eval_origin: if reader.bool()? { Some(JsString::read_snapshot(reader)?) } else { None },
    })
}

fn write_stack_data(writer: &mut Writer, stack: &StackData) -> SnapshotResult<()> {
    match stack {
        StackData::Captured { records, source } => {
            writer.u8(0);
            writer.len(records.len());
            for record in records {
                write_call_site(writer, record);
            }
            writer.bool(source.is_some());
            if let Some(source) = source {
                source.write_snapshot(writer);
            }
        }
        StackData::Formatted(value) => {
            writer.u8(1);
            writer.value(*value);
        }
        StackData::CallSite(record) => {
            writer.u8(2);
            write_call_site(writer, record);
        }
    }
    Ok(())
}

fn read_stack_data(reader: &mut Reader<'_>) -> SnapshotResult<StackData> {
    Ok(match reader.u8()? {
        0 => StackData::Captured {
            records: read_list(reader, read_call_site)?,
            source: if reader.bool()? { Some(crate::bytecode::ProgramSource::read_snapshot(reader)?) } else { None },
        },
        1 => StackData::Formatted(reader.value()?),
        2 => StackData::CallSite(read_call_site(reader)?),
        _ => return Err(SnapshotError("invalid snapshot stack data".into())),
    })
}

impl ArrayObject {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        write_object(writer, &self.object)?;
        writer.values(self.elements());
        Ok(())
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let object = read_object(reader)?;
        if !object.is_array() {
            return Err(SnapshotError("snapshot array without its array flag".into()));
        }
        let elements = Rc::new(reader.values()?);
        let mut array = Self { object };
        // SAFETY: `read_object` left the second word zero; this writes the backing there without
        // reading or dropping it, as `new` does.
        unsafe {
            std::ptr::write(
                std::ptr::addr_of_mut!((*array.object.payload.array).elements),
                std::mem::ManuallyDrop::new(elements),
            );
        }
        Ok(array)
    }
}

impl Object {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        write_object(writer, self)
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        read_object(reader)
    }
}

impl FunctionRecord {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        let Self { object, realm, env, kind } = self;
        write_object(writer, object)?;
        writer.value(*realm);
        writer.value(*env);
        kind.write_snapshot(writer);
        Ok(())
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Self {
            object: read_object(reader)?,
            realm: reader.value()?,
            env: reader.value()?,
            kind: FunctionKind::read_snapshot(reader)?,
        })
    }
}

impl IteratorRecord {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        let Self { object, source, kind, index, done, ext } = self;
        let IteratorExt { position_high, next_method, helper, helper_running, helper_started, generator } = ext;
        if helper.is_some() || generator.is_some() {
            return Err(SnapshotError::unsupported("a suspended iterator helper or generator"));
        }
        write_object(writer, object)?;
        writer.value(*source);
        iterator_kind::write(writer, *kind);
        writer.u32(*index);
        writer.bool(*done);
        writer.u32(*position_high);
        writer.option_value(*next_method);
        writer.bool(*helper_running);
        writer.bool(*helper_started);
        Ok(())
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Self {
            object: read_object(reader)?,
            source: reader.value()?,
            kind: iterator_kind::read(reader)?,
            index: reader.u32()?,
            done: reader.bool()?,
            ext: IteratorExt {
                position_high: reader.u32()?,
                next_method: reader.option_value()?,
                helper: None,
                helper_running: reader.bool()?,
                helper_started: reader.bool()?,
                generator: None,
            },
        })
    }
}

impl crate::heap::PromiseCell {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        let crate::heap::PromiseRecord { state, result, reactions, rejection, rejection_id, host_context } =
            &self.record;
        write_object(writer, &self.object)?;
        promise_state::write(writer, *state);
        writer.value(*result);
        writer.len(reactions.len());
        for reaction in reactions {
            writer.value(reaction.on_fulfilled);
            writer.value(reaction.on_rejected);
            writer.value(reaction.next);
            writer.bool(reaction.execution_context.is_some());
            if let Some(context) = reaction.execution_context {
                writer.u64(context.0);
            }
        }
        promise_rejection::write(writer, *rejection);
        writer.u64(rejection_id.map_or(0, std::num::NonZeroU64::get));
        writer.bool(host_context.is_some());
        if let Some(context) = host_context {
            writer.u64(context.0);
        }
        Ok(())
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let object = read_object(reader)?;
        let state = promise_state::read(reader)?;
        let result = reader.value()?;
        let reactions = read_list(reader, |reader| {
            Ok(crate::heap::PromiseReaction {
                on_fulfilled: reader.value()?,
                on_rejected: reader.value()?,
                next: reader.value()?,
                execution_context: if reader.bool()? { Some(crate::HostExecutionContext(reader.u64()?)) } else { None },
            })
        })?;
        let rejection = promise_rejection::read(reader)?;
        let rejection_id = std::num::NonZeroU64::new(reader.u64()?);
        let host_context = if reader.bool()? { Some(crate::HostExecutionContext(reader.u64()?)) } else { None };
        Ok(Self {
            object,
            record: crate::heap::PromiseRecord { state, result, reactions, rejection, rejection_id, host_context },
        })
    }
}

impl Cell {
    /// Writes this cell, or refuses one a snapshot cannot hold.
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        match self {
            Self::Object(object) => {
                writer.u8(tag::OBJECT);
                write_object(writer, object)?;
            }
            Self::Array { object } => {
                writer.u8(tag::ARRAY);
                object.write_snapshot(writer)?;
            }
            Self::ArrayBuffer { object, bytes, shared, detached, max_byte_length, resizable, immutable } => {
                let Some(contents) = bytes.snapshot_contents() else {
                    return Err(SnapshotError::unsupported("a buffer over Wasm, shared or pinned memory"));
                };
                writer.u8(tag::ARRAY_BUFFER);
                write_object(writer, object)?;
                writer.bytes(contents);
                writer.bool(*shared);
                writer.bool(*detached);
                writer.u64(*max_byte_length as u64);
                writer.bool(*resizable);
                writer.bool(*immutable);
            }
            Self::TypedArray { kind, object, buffer, length, length_tracking } => {
                writer.u8(tag::TYPED_ARRAY);
                typed_array_kind::write(writer, *kind);
                write_object(writer, &object.object)?;
                writer.u64(object.offset as u64);
                writer.value(*buffer);
                writer.u64(*length as u64);
                writer.bool(*length_tracking);
            }
            Self::DataView { object, buffer, length, length_tracking } => {
                writer.u8(tag::DATA_VIEW);
                write_object(writer, &object.object)?;
                writer.u64(object.offset as u64);
                writer.value(*buffer);
                writer.u64(*length as u64);
                writer.bool(*length_tracking);
            }
            Self::Map { object, entries } => {
                writer.u8(tag::MAP);
                write_object(writer, object)?;
                let (slots, live, iterators) = entries.snapshot_parts()?;
                writer.len(slots.len());
                for (key, value) in slots {
                    writer.value(*key);
                    writer.value(*value);
                }
                writer.len(live);
                writer.values(iterators);
            }
            Self::Set { object, entries } => {
                writer.u8(tag::SET);
                write_object(writer, object)?;
                let (slots, live, iterators) = entries.snapshot_parts()?;
                writer.values(slots);
                writer.len(live);
                writer.values(iterators);
            }
            Self::ShadowRealm { object, caller_global, realm_global } => {
                writer.u8(tag::SHADOW_REALM);
                write_object(writer, object)?;
                writer.value(*caller_global);
                writer.value(*realm_global);
            }
            Self::WeakMap { object, entries } => {
                writer.u8(tag::WEAK_MAP);
                write_object(writer, object)?;
                writer.len(entries.ordered.len());
                for (key, value) in &entries.ordered {
                    writer.value(*key);
                    writer.value(*value);
                }
            }
            Self::WeakSet { object, entries } => {
                writer.u8(tag::WEAK_SET);
                write_object(writer, object)?;
                writer.values(entries);
            }
            Self::WeakRef { object, target } => {
                writer.u8(tag::WEAK_REF);
                write_object(writer, object)?;
                writer.option_value(target.map(|handle| handle.value));
            }
            Self::FinalizationRegistry { object, callback, entries } => {
                writer.u8(tag::FINALIZATION_REGISTRY);
                write_object(writer, object)?;
                writer.value(*callback);
                writer.len(entries.len());
                for entry in entries.iter() {
                    writer.value(entry.target.value);
                    writer.value(entry.held);
                    writer.option_value(entry.token.map(|handle| handle.value));
                }
            }
            Self::Iterator(iterator) => {
                writer.u8(tag::ITERATOR);
                iterator.write_snapshot(writer)?;
            }
            Self::Proxy { object, kind, target, handler } => {
                writer.u8(tag::PROXY);
                write_object(writer, object)?;
                proxy_kind::write(writer, *kind);
                writer.value(*target);
                writer.value(*handler);
            }
            Self::Function(function) => {
                writer.u8(tag::FUNCTION);
                function.write_snapshot(writer)?;
            }
            Self::BindingReference { environment, slot, kind } => {
                writer.u8(tag::BINDING_REFERENCE);
                writer.value(*environment);
                writer.u16(*slot);
                lexical_binding_kind::write(writer, *kind);
            }
            Self::Environment { parent, function, slots, scope } => {
                writer.u8(tag::ENVIRONMENT);
                writer.value(*parent);
                writer.u32(*function);
                writer.bool(slots.0.marked());
                writer.len(slots.0.len());
                for slot in slots.0.iter() {
                    writer.value(slot.0);
                }
                scope.write_snapshot(writer);
            }
            Self::String(text) => {
                writer.u8(tag::STRING);
                text.write_snapshot(writer);
            }
            Self::BigInt(value) => {
                writer.u8(tag::BIGINT);
                writer.bytes(&value.to_signed_bytes_le());
            }
            Self::Symbol(SymbolData { description, kind }) => {
                writer.u8(tag::SYMBOL);
                writer.option_str(description.as_deref());
                symbol_kind::write(writer, *kind);
            }
            Self::Date { milliseconds, object } => {
                writer.u8(tag::DATE);
                writer.f64(*milliseconds);
                write_object(writer, object)?;
            }
            Self::RegExp { object, meta, matcher: _ } => {
                writer.u8(tag::REGEXP);
                write_object(writer, object)?;
                let RegExpMeta { source, flags, legacy_constructor } = &**meta;
                source.write_snapshot(writer);
                writer.str(flags);
                match legacy_constructor {
                    RegExpLegacyOwner::Enabled(value) => {
                        writer.u8(0);
                        writer.value(*value);
                    }
                    RegExpLegacyOwner::Disabled(value) => {
                        writer.u8(1);
                        writer.value(*value);
                    }
                }
            }
            Self::Error(message) => {
                writer.u8(tag::ERROR);
                writer.str(message);
            }
            Self::Promise(promise) => {
                writer.u8(tag::PROMISE);
                promise.write_snapshot(writer)?;
            }
            Self::PromiseResolvingState { promise, already_resolved } => {
                writer.u8(tag::PROMISE_RESOLVING_STATE);
                writer.value(*promise);
                writer.bool(*already_resolved);
            }
            Self::ArrayFromAsyncState(_) | Self::AwaitState(_) => {
                return Err(SnapshotError::unsupported("a suspended async operation"));
            }
            Self::WasmBits64(_)
            | Self::WasmV128(_)
            | Self::WasmExtern(_)
            | Self::WasmException { .. }
            | Self::WasmTag { .. }
            | Self::WasmGc { .. }
            | Self::WasmHostFunction { .. }
            | Self::WasmElements(_)
            | Self::WasmObject { .. }
            | Self::WasmGlobal { .. }
            | Self::WasmMemory { .. }
            | Self::WasmTable { .. } => return Err(SnapshotError::unsupported("WebAssembly")),
            Self::TemporalDuration { .. }
            | Self::TemporalPlainDate { .. }
            | Self::TemporalPlainDateTime { .. }
            | Self::TemporalPlainMonthDay { .. }
            | Self::TemporalPlainYearMonth { .. }
            | Self::TemporalZonedDateTime { .. }
            | Self::TemporalInstant { .. } => return Err(SnapshotError::unsupported("a Temporal object")),
        }
        Ok(())
    }

    pub(crate) fn read_snapshot(reader: &mut Reader<'_>, decoder: &mut CellDecoder<'_>) -> SnapshotResult<Self> {
        let boxed_object = |reader: &mut Reader<'_>| read_object(reader).map(Box::new);
        Ok(match reader.u8()? {
            tag::OBJECT => Self::Object(read_object(reader)?),
            tag::ARRAY => Self::Array { object: ArrayObject::read_snapshot(reader)? },
            tag::ARRAY_BUFFER => Self::ArrayBuffer {
                object: boxed_object(reader)?,
                bytes: crate::heap::BufferBytes::from(reader.bytes()?.to_vec()),
                shared: reader.bool()?,
                detached: reader.bool()?,
                max_byte_length: reader.u64()? as usize,
                resizable: reader.bool()?,
                immutable: reader.bool()?,
            },
            tag::TYPED_ARRAY => {
                let kind = typed_array_kind::read(reader)?;
                Self::TypedArray {
                    kind,
                    object: Box::new(BufferViewObject { object: read_object(reader)?, offset: reader.u64()? as usize }),
                    buffer: reader.value()?,
                    length: reader.u64()? as usize,
                    length_tracking: reader.bool()?,
                }
            }
            tag::DATA_VIEW => Self::DataView {
                object: Box::new(BufferViewObject { object: read_object(reader)?, offset: reader.u64()? as usize }),
                buffer: reader.value()?,
                length: reader.u64()? as usize,
                length_tracking: reader.bool()?,
            },
            tag::MAP => {
                let object = boxed_object(reader)?;
                let slots = read_list(reader, |reader| Ok((reader.value()?, reader.value()?)))?;
                let live = reader.len()?;
                let iterators = reader.values()?;
                Self::Map { object, entries: Box::new(crate::heap::CollectionEntries::from_snapshot_parts(slots, live, iterators)) }
            }
            tag::SET => {
                let object = boxed_object(reader)?;
                let slots = reader.values()?;
                let live = reader.len()?;
                let iterators = reader.values()?;
                Self::Set { object, entries: Box::new(crate::heap::CollectionEntries::from_snapshot_parts(slots, live, iterators)) }
            }
            tag::SHADOW_REALM => Self::ShadowRealm {
                object: boxed_object(reader)?,
                caller_global: reader.value()?,
                realm_global: reader.value()?,
            },
            tag::WEAK_MAP => {
                let object = boxed_object(reader)?;
                let ordered = read_list(reader, |reader| Ok((reader.value()?, reader.value()?)))?;
                let indices = ordered.iter().enumerate().map(|(index, (key, _))| (*key, index)).collect();
                Self::WeakMap { object, entries: Box::new(WeakMapEntries { ordered, indices }) }
            }
            tag::WEAK_SET => Self::WeakSet { object: boxed_object(reader)?, entries: Box::new(reader.values()?) },
            tag::WEAK_REF => Self::WeakRef {
                object: boxed_object(reader)?,
                target: reader.option_value()?.map(|value| WeakHandle { value }),
            },
            tag::FINALIZATION_REGISTRY => {
                let object = boxed_object(reader)?;
                let callback = reader.value()?;
                let entries = read_list(reader, |reader| {
                    Ok(FinalizationEntry {
                        target: WeakHandle { value: reader.value()? },
                        held: reader.value()?,
                        token: reader.option_value()?.map(|value| WeakHandle { value }),
                    })
                })?;
                Self::FinalizationRegistry { object, callback, entries: Box::new(FinalizationEntries(entries)) }
            }
            tag::ITERATOR => Self::Iterator(Box::new(IteratorRecord::read_snapshot(reader)?)),
            tag::PROXY => Self::Proxy {
                object: boxed_object(reader)?,
                kind: proxy_kind::read(reader)?,
                target: reader.value()?,
                handler: reader.value()?,
            },
            tag::FUNCTION => Self::Function(Box::new(FunctionRecord::read_snapshot(reader)?)),
            tag::BINDING_REFERENCE => Self::BindingReference {
                environment: reader.value()?,
                slot: reader.u16()?,
                kind: lexical_binding_kind::read(reader)?,
            },
            tag::ENVIRONMENT => {
                let parent = reader.value()?;
                let function = reader.u32()?;
                let marked = reader.bool()?;
                let values = reader.values()?;
                let mut slots = crate::heap::ThinBoxSlice::from(values.into_iter().map(EnvironmentSlot).collect::<Vec<_>>());
                if marked {
                    slots.mark();
                }
                Self::Environment { parent, function, slots: EnvironmentSlots(slots), scope: EnvironmentScopeRef::read_snapshot(reader)? }
            }
            tag::STRING => Self::String(JsString::read_snapshot(reader)?),
            tag::BIGINT => Self::BigInt(Box::new(num_bigint::BigInt::from_signed_bytes_le(reader.bytes()?))),
            tag::SYMBOL => Self::Symbol(SymbolData {
                description: reader.option_string()?.map(String::into_boxed_str),
                kind: symbol_kind::read(reader)?,
            }),
            tag::DATE => Self::Date { milliseconds: reader.f64()?, object: boxed_object(reader)? },
            tag::REGEXP => {
                let object = boxed_object(reader)?;
                let source = JsString::read_snapshot(reader)?;
                let flags = reader.string()?;
                let legacy_constructor = match reader.u8()? {
                    0 => RegExpLegacyOwner::Enabled(reader.value()?),
                    1 => RegExpLegacyOwner::Disabled(reader.value()?),
                    _ => return Err(SnapshotError("invalid snapshot regexp owner".into())),
                };
                let matcher = (decoder.regexp)(&source, &flags)?;
                Self::RegExp { object, meta: Box::new(RegExpMeta { source, flags, legacy_constructor }), matcher }
            }
            tag::ERROR => Self::Error(reader.string()?),
            tag::PROMISE => Self::Promise(Box::new(crate::heap::PromiseCell::read_snapshot(reader)?)),
            tag::PROMISE_RESOLVING_STATE => Self::PromiseResolvingState { promise: reader.value()?, already_resolved: reader.bool()? },
            _ => return Err(SnapshotError("invalid snapshot cell".into())),
        })
    }
}

impl EnvironmentScopeRef {
    fn write_snapshot(&self, writer: &mut Writer) {
        match self.scope() {
            None => {
                writer.u8(0);
                writer.bool(self.program().is_some());
                writer.u32(self.program().unwrap_or(0));
            }
            Some(EnvironmentScope { program, root_eval_scope, binding_site_pc, dynamic_bindings, with_objects }) => {
                writer.u8(1);
                writer.bool(program.is_some());
                writer.u32(program.unwrap_or(0));
                writer.bool(*root_eval_scope);
                writer.bool(binding_site_pc.is_some());
                writer.u32(binding_site_pc.unwrap_or(0));
                match dynamic_bindings {
                    EnvironmentBindings::Owned(bindings) => {
                        writer.u8(0);
                        writer.len(bindings.len());
                        for (atom, value) in bindings {
                            writer.u32(*atom);
                            writer.value(*value);
                        }
                    }
                    EnvironmentBindings::Shared(value) => {
                        writer.u8(1);
                        writer.value(*value);
                    }
                }
                writer.values(with_objects);
            }
        }
    }

    fn read_snapshot(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let tag = reader.u8()?;
        let has_program = reader.bool()?;
        let program = reader.u32()?;
        let program = has_program.then_some(program);
        match tag {
            0 => Self::inline(program).ok_or_else(|| SnapshotError("invalid snapshot inline scope".into())),
            1 => {
                let root_eval_scope = reader.bool()?;
                let has_pc = reader.bool()?;
                let pc = reader.u32()?;
                let dynamic_bindings = match reader.u8()? {
                    0 => EnvironmentBindings::Owned(read_list(reader, |reader| Ok((reader.u32()?, reader.value()?)))?),
                    1 => EnvironmentBindings::Shared(reader.value()?),
                    _ => return Err(SnapshotError("invalid snapshot scope bindings".into())),
                };
                let with_objects = reader.values()?.into_boxed_slice();
                Ok(Self::boxed(EnvironmentScope {
                    program,
                    root_eval_scope,
                    binding_site_pc: has_pc.then_some(pc),
                    dynamic_bindings,
                    with_objects,
                }))
            }
            _ => Err(SnapshotError("invalid snapshot scope".into())),
        }
    }
}
