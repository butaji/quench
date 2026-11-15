//! The edges of each cell kind: what marking traces from a cell, and the cell kind census.
use super::*;

impl Heap {
    /// Push the values `object` keeps alive. Objects that share a prototype push it once:
    /// `last_proto`, the prototypes pushed last in this walk, are already on the work list or
    /// marked, so a second push would only be popped again.
    #[inline(always)]
    pub(super) fn object_children(
        owner: Value,
        object: &Object,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        Self::object_children_marking(owner, object, properties, |_, _, _| false, work, object_roots, trace_kind, last_proto);
    }

    /// `object_children`, marking each inline string value through `mark_string` where the
    /// trace finds it (`Heap::mark_leaf`), as an array's string elements are: a string's only
    /// edges are its parts, which `mark_string` marks too, so it never enters the work list
    /// (V8-v7 Splay: a string per leaf object, a sixth of a full collection's pops).
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn object_children_marking(
        owner: Value,
        object: &Object,
        properties: &ValueArena,
        mut mark_leaf: impl FnMut(Value, &mut Vec<Value>, &mut PushedProtos) -> bool,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        // The common object: inline values, no extras. Its prototype is tested by link first,
        // and its values by narrow code, before anything is decoded. One loop takes either
        // kind of inline values, so `mark_leaf` stays inlined at a single site.
        let inline = if let Some((link, narrows)) = object.plain_inline_trace() {
            if last_proto.first_link(link) {
                let proto = link.prototype();
                if proto.is_heap() && last_proto.first_push(proto) {
                    work.push(proto);
                }
            }
            object_roots(trace_kind, owner, object.shape(), false, work);
            Some(narrows.map(|narrow| if Value::narrow_is_heap(narrow) { Value::from_narrow(narrow) } else { Value::UNDEFINED }))
        } else {
            let proto = object.proto();
            if proto.is_heap() && last_proto.first_push(proto) {
                work.push(proto);
            }
            object_roots(trace_kind, owner, object.shape(), object.was_used_as_prototype(), work);
            object.visit_extras_roots(|value| work.push(value));
            object.inline_properties()
        };
        if let Some(values) = inline {
            for value in values {
                if value.is_heap() && !value.is_deleted() && !mark_leaf(value, work, last_proto) {
                    work.push(value);
                }
            }
        } else {
            properties.append_heap_references(object.properties(), work);
        }
    }

    pub(super) fn children(
        owner: Value,
        cell: &Cell,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        let mut object = |object: &Object| {
            Self::object_children(
                owner,
                object,
                properties,
                work,
                object_roots,
                trace_kind,
                last_proto,
            );
        };
        if let Some((value, buffer)) = cell.typed_array_backing() {
            object(value);
            work.push(buffer);
            return;
        }
        match cell {
            Cell::Object(value) => object(value),
            Cell::Array { object: value } => {
                object(value);
                work.extend(
                    value
                        .elements()
                        .iter()
                        
                        .filter(|value| value.is_heap()),
                );
            }
            Cell::ArrayBuffer { object: value, .. } => object(value),
            Cell::RegExp {
                object: value,
                meta,
                ..
            } => {
                object(value);
                work.push(meta.legacy_constructor.constructor());
            }
            Cell::DataView {
                object: value,
                buffer,
                ..
            } => {
                object(value);
                work.push(*buffer);
            }
            Cell::Map {
                object: value,
                entries,
            } => {
                object(value);
                work.extend(entries.iter().flat_map(|(key, value)| [key, value]));
            }
            Cell::Set {
                object: value,
                entries,
            } => {
                object(value);
                work.extend(entries.iter());
            }
            Cell::ShadowRealm {
                object: value,
                caller_global,
                realm_global,
            } => {
                object(value);
                work.extend([*caller_global, *realm_global]);
            }
            Cell::WeakMap { object: value, .. } | Cell::WeakSet { object: value, .. } => {
                object(value);
            }
            Cell::WeakRef { object: value, .. } => object(value),
            Cell::FinalizationRegistry {
                object: value,
                callback,
                entries,
            } => {
                object(value);
                work.push(*callback);
                work.extend(entries.iter().map(|entry| entry.held));
            }
            Cell::Iterator(iterator) => {
                object(&iterator.object);
                Self::iterator_edges(iterator, work);
            }
            Cell::AwaitState(state) => state.extend_roots(work),
            Cell::ArrayFromAsyncState(state) => {
                work.extend([state.output, state.result, state.this_arg]);
                work.extend(state.iterator);
                work.extend(state.mapper);
                work.extend(state.array_like.map(|(source, _)| source));
            }
            Cell::Proxy {
                object: value,
                target,
                handler,
                ..
            } => {
                object(value);
                work.extend([*target, *handler]);
            }
            Cell::Function(function) => {
                object(&function.object);
                work.extend([function.env, function.realm]);
            }
            Cell::WasmGlobal { header, bits, ty, .. } => {
                object(&header.object);
                work.extend((WasmStorage::of(*ty) == WasmStorage::Handle).then(|| Value::from_bits(*bits)));
            }
            Cell::BindingReference { environment, .. } => work.push(*environment),
            Cell::Environment {
                parent,
                slots,
                scope,
                ..
            } => {
                work.push(*parent);
                work.extend(slots.roots());
                let (dynamic_bindings, with_objects) = (scope.dynamic_bindings(), scope.with_objects());
                match dynamic_bindings {
                    EnvironmentBindings::Owned(bindings) => {
                        work.extend(bindings.iter().map(|(_, value)| *value));
                    }
                    EnvironmentBindings::Shared(owner) => work.push(*owner),
                }
                work.extend(with_objects.iter().copied());
            }
            Cell::WasmElements(elements) => work.extend(elements.iter().copied()),
            Cell::WasmTable { object: value, elements, .. } => {
                object(value);
                work.extend(elements.iter().copied());
            }
            Cell::WasmGc { fields, .. } => work.extend(fields.edges().iter().copied()),
            Cell::WasmExtern(value) => work.push(*value),
            Cell::AccessorPair { getter, setter } => {
                work.push(*getter);
                work.push(*setter);
            }
            Cell::WasmException { tag, payload } => {
                work.push(*tag);
                work.extend(payload.iter().copied());
            }
            Cell::PromiseResolvingState { promise, .. } => work.push(*promise),
            Cell::Promise(promise) => {
                object(&promise.object);
                promise.record.append_roots(work);
            }
            Cell::Date { object: value, .. }
            | Cell::TemporalDuration { object: value, .. }
            | Cell::TemporalPlainDate { object: value, .. }
            | Cell::TemporalPlainDateTime { object: value, .. }
            | Cell::TemporalPlainMonthDay { object: value, .. }
            | Cell::TemporalPlainYearMonth { object: value, .. }
            | Cell::TemporalZonedDateTime { object: value, .. }
            | Cell::TemporalInstant { object: value, .. }
            | Cell::WasmTag { object: value, .. } => object(value),
            Cell::BigInt(_)
            | Cell::Symbol(_)
            | Cell::Error(_)
            | Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::Host(_),
                ..
            }
            | Cell::WasmBits64(_) | Cell::WasmInstance(_) | Cell::WasmV128(_) => {}
            Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::Js(target),
                ..
            } => work.push(*target),
            Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target),
                ..
            } => {
                work.push(target.function);
                work.push(target.view_environment);
            }
            Cell::WasmObject { object: value, slot } => {
                object(value);
                if let crate::heap::WasmObjectSlot::Instance { exports } = slot {
                    work.push(*exports);
                }
            }
            Cell::WasmMemory { header, buffer, .. } => {
                object(&header.object);
                work.push(*buffer);
            }
            _ => unreachable!("typed array backing handled above"),
        }
    }

    /// A direct Array record traces exactly as a legacy `Cell::Array`: its object part, then
    /// its heap-valued elements.
    // Inlined into each mark visit, as the serial mark had it: left to its heuristics, LLVM
    // calls it from `collect_with_object_roots`, which cachegrind measured at more instructions.
    #[inline(always)]
    pub(super) fn array_children(
        owner: Value,
        array: &ArrayObject,
        properties: &ValueArena,
        mut mark_string: impl FnMut(RecordId) -> bool,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        Self::object_children(
            owner,
            array,
            properties,
            work,
            object_roots,
            trace_kind,
            last_proto,
        );
        // The count of non-int32 values is kept per backing: none means no element to scan.
        if array.elements().holds_only_int32() || array.elements().double_values().is_some() { return; }
        match array.elements().wide_values() {
            Some(values) => work.extend(
                values.iter().copied().filter(|value| value.is_heap() && !Self::mark_leaf(&mut mark_string, *value)),
            ),
            None => work.extend(
                array.elements().iter().filter(|value| value.is_heap() && !Self::mark_leaf(&mut mark_string, *value)),
            ),
        }
    }

    /// Marks `value` where the trace finds it, through `mark_string`, when it is a string, and
    /// says whether it was one. A string holds no heap edges, so marking it is the whole visit
    /// (`mark_value`): an array of strings adds nothing to the mark stack, which had held a
    /// value per element at the heap's peak (V8-v7 RegExp: 141 KiB). Profiling builds leave
    /// every value to `mark_value`, which counts each visit.
    pub(super) fn mark_leaf(mark_string: &mut impl FnMut(RecordId) -> bool, value: Value) -> bool {
        if cfg!(feature = "profile-aggregate") {
            return false;
        }
        let Some(record_id) = value.direct_string_index() else {
            return false;
        };
        mark_string(record_id)
    }

    /// An iterator's edges besides its object: its source, cached next method, helper
    /// state and suspended generator.
    pub(super) fn iterator_edges(iterator: &IteratorRecord, work: &mut Vec<Value>) {
        let IteratorExt {
            next_method,
            helper,
            generator,
            ..
        } = &iterator.ext;
        work.push(iterator.source);
        work.extend(*next_method);
        if let Some(helper) = helper {
            match helper.as_ref() {
                IteratorHelper::Map { callback, .. }
                | IteratorHelper::Filter { callback, .. }
                | IteratorHelper::FlatMap { callback, .. } => work.push(*callback),
                IteratorHelper::RegExpStringMatchAll { input, .. } => work.push(*input),
                IteratorHelper::Take { .. } | IteratorHelper::Drop { .. } => {}
                IteratorHelper::Concat {
                    items,
                    methods,
                    opened,
                    active,
                    ..
                } => {
                    work.extend(items.iter().copied());
                    work.extend(methods.iter().copied());
                    work.extend(opened.iter().flatten().copied());
                    work.extend(*active);
                }
                IteratorHelper::Zip {
                    iterators,
                    padding,
                    keys,
                    ..
                } => {
                    work.extend(iterators.iter().copied());
                    work.extend(padding.iter().copied());
                    work.extend(keys.iter().flatten().copied());
                }
            }
            match helper.as_ref() {
                IteratorHelper::FlatMap {
                    inner: Some(inner), ..
                } => work.push(*inner),
                IteratorHelper::FlatMap { inner: None, .. } => {}
                IteratorHelper::Zip { .. }
                | IteratorHelper::Map { .. }
                | IteratorHelper::Filter { .. }
                | IteratorHelper::Take { .. }
                | IteratorHelper::Drop { .. }
                | IteratorHelper::Concat { .. }
                | IteratorHelper::RegExpStringMatchAll { .. } => {}
            }
        }
        if let Some(record) = generator {
            record.extend_roots(work);
        }
    }

    /// A function's edges: its object, closure environment and realm.
    // Inlined into each mark visit, as `array_children`.
    #[inline(always)]
    pub(super) fn function_children(
        owner: Value,
        function: &FunctionRecord,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut PushedProtos,
    ) {
        Self::object_children(
            owner,
            &function.object,
            properties,
            work,
            object_roots,
            trace_kind,
            last_proto,
        );
        // The environment edge is the caller's (`EnvironmentTrace::closure`).
        work.push(function.realm);
    }
}
