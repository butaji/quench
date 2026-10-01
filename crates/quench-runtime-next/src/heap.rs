use crate::value::Value;
use crate::value_vec::ValueArena;
use rustc_hash::FxHashMap;
mod access;
mod cell;
mod cell_access;
mod external;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod root;
mod slots;
mod weak;
pub(crate) use cell::*;
pub use root::RootId;
pub(crate) use root::{RootTable, WeakHandle};
use slots::SlotArena;
pub(super) struct Slot {
    cell: Option<Cell>,
}
#[derive(Default)]
pub(crate) struct Heap {
    slots: SlotArena,
    marks: Vec<u64>,
    free: Vec<u32>,
    retired_slots: usize,
    generations: Vec<u32>,
    external_bytes: usize,
    allocations: usize,
    threshold: usize,
    total_allocations: u64,
    collections: u64,
    peak_live: usize,
    peak_survivors: usize,
    max_threshold: usize,
    properties: ValueArena,
    roots: RootTable,
    sparse_arrays: Option<Box<FxHashMap<u32, SparseElements>>>,
    #[cfg(feature = "profile-aggregate")]
    gc_profile: GcProfile,
    #[cfg(feature = "profile-memory")]
    memory_profile: memory_profile::MemoryProfile,
}
#[cfg(feature = "profile-aggregate")]
#[derive(Clone, Copy, Default)]
pub(crate) struct GcProfile {
    pub allocated_kinds: [u64; CellKind::COUNT],
    pub allocated_payload_bytes: [u64; CellKind::COUNT],
    pub allocated_size_buckets: [[u64; 8]; CellKind::COUNT],
    pub roots: u64,
    pub work_items: u64,
    pub max_worklist: u64,
    pub marked: u64,
    pub freed: u64,
    pub sweep_slots: u64,
    pub mark_nanos: u64,
    pub sweep_nanos: u64,
    pub marked_kinds: [u64; CellKind::COUNT],
}
#[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
#[repr(usize)]
#[derive(Clone, Copy)]
pub(crate) enum CellKind {
    Object,
    Array,
    Map,
    Set,
    Iterator,
    WeakMap,
    WeakSet,
    WeakRef,
    Function,
    Environment,
    String,
    BigInt,
    Symbol,
    Date,
    Error,
    RegExp,
    ArrayFromAsync,
    TemporalDuration,
    TemporalPlainDate,
    TemporalPlainDateTime,
    TemporalPlainMonthDay,
    TemporalPlainYearMonth,
    TemporalZonedDateTime,
    TemporalInstant,
    WasmBits64,
}
#[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
impl CellKind {
    pub(crate) const COUNT: usize = Self::WasmBits64 as usize + 1;
    #[cfg(feature = "profile-memory")]
    pub(crate) const NAMES: [&'static str; Self::COUNT] = [
        "object",
        "array",
        "map",
        "set",
        "iterator",
        "weak_map",
        "weak_set",
        "weak_ref",
        "function",
        "environment",
        "string",
        "bigint",
        "symbol",
        "date",
        "error",
        "regexp",
        "array_from_async",
        "temporal_duration",
        "temporal_plain_date",
        "temporal_plain_date_time",
        "temporal_plain_month_day",
        "temporal_plain_year_month",
        "temporal_zoned_date_time",
        "temporal_instant",
        "wasm_bits64",
    ];
}
#[derive(Default)]
struct SparseElements {
    values: FxHashMap<usize, Value>,
    length: usize,
}
impl Heap {
    pub fn new() -> Self {
        Self {
            slots: SlotArena::with_small_capacity(),
            marks: Vec::with_capacity(12),
            free: Vec::with_capacity(384),
            threshold: 384,
            max_threshold: 384,
            ..Self::default()
        }
    }
    /// Keep the complete allocation census available to ownership tests.
    /// Explicit collection remains available; only threshold collection stops.
    #[cfg(test)]
    pub(crate) fn retain_allocations_for_test(&mut self) {
        self.threshold = usize::MAX;
    }

    #[cfg(test)]
    pub(crate) fn environment_count_for_test(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| matches!(slot.cell, Some(Cell::Environment { .. })))
            .count()
    }

    #[cfg(test)]
    pub(crate) fn root_count_for_test(&self) -> usize {
        self.roots.values().count()
    }

    pub fn alloc(&mut self, cell: Cell) -> Value {
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = Self::cell_kind(&cell) as usize;
            let bytes = Self::cell_payload_bytes(&cell);
            self.gc_profile.allocated_kinds[kind] += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][Self::size_bucket(bytes)] += 1;
        }
        self.allocations += 1;
        self.total_allocations += 1;
        self.external_bytes += cell.external_bytes();
        while let Some(index) = self.free.pop() {
            let Some(generation) = self.generations[index as usize].checked_add(1) else {
                self.retired_slots += 1;
                continue;
            };
            if let Some(arrays) = &mut self.sparse_arrays {
                arrays.remove(&index);
            }
            self.generations[index as usize] = generation;
            self.slots.get_mut(index as usize).unwrap().cell = Some(cell);
            #[cfg(feature = "profile-memory")]
            self.memory_profile.allocated(
                index as usize,
                self.slots
                    .get(index as usize)
                    .unwrap()
                    .cell
                    .as_ref()
                    .unwrap(),
            );
            self.peak_live = self
                .peak_live
                .max(self.slots.len() - self.free.len() - self.retired_slots);
            return Value::heap(index);
        }
        let index = self.slots.len();
        self.slots.push(Slot { cell: Some(cell) });
        #[cfg(feature = "profile-memory")]
        self.memory_profile
            .allocated(index, self.slots.get(index).unwrap().cell.as_ref().unwrap());
        if index / 64 == self.marks.len() {
            self.marks.push(0);
        }
        self.generations.push(1);
        self.peak_live = self
            .peak_live
            .max(self.slots.len() - self.free.len() - self.retired_slots);
        Value::heap(index as u32)
    }
    pub(crate) fn alloc_object_pair(
        &mut self,
        proto: Value,
        shape: u32,
        first: Value,
        second: Value,
    ) -> Value {
        let properties = self.properties.pair(shape, first, second);
        self.alloc(Cell::Object(Object::new(proto, properties)))
    }
    pub(crate) fn register_property_shape(&mut self, shape: u32, length: usize) {
        self.properties.register_shape(shape, length);
    }
    pub(crate) fn live_object_shapes(&self) -> Vec<u32> {
        self.slots
            .iter()
            .filter_map(|slot| slot.cell.as_ref()?.object().map(Object::shape))
            .collect()
    }
    pub(crate) fn remap_live_object_shapes(&mut self, mapping: &[u32], lengths: &[usize]) {
        for slot in self.slots.iter_mut() {
            let Some(object) = slot.cell.as_mut().and_then(Cell::object_mut) else {
                continue;
            };
            let old_shape = object.shape() as usize;
            object.set_shape(mapping[old_shape]);
        }
        self.properties.reset_shapes();
        for (shape, length) in lengths.iter().copied().enumerate() {
            self.properties.register_shape(shape as u32, length);
        }
    }
    pub(crate) fn compact_property_arena(&mut self) {
        if !self.properties.has_released_ranges() {
            return;
        }
        #[cfg(feature = "profile-memory")]
        let before = self.properties.stats();
        let mut objects = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let object = slot.cell.as_ref()?.object()?;
                self.properties
                    .has_compact_range(object.properties)
                    .then_some(())?;
                Some((object.properties.start_offset(), index))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable();
        let mut target = 0;
        for (_, index) in objects {
            let slot = self
                .slots
                .get_mut(index)
                .expect("property owner slot exists");
            let object = slot
                .cell
                .as_mut()
                .and_then(Cell::object_mut)
                .expect("property owner remains live during compaction");
            target += self
                .properties
                .compact_vector(&mut object.properties, target);
        }
        self.properties.finish_compaction(target);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            let after = self.properties.stats();
            eprintln!(
                "{{\"kind\":\"rqj-property-compaction\",\"values_before\":{},\"values_after\":{},\"capacity_before\":{},\"capacity_after\":{},\"free_ranges_before\":{},\"free_ranges_after\":{}}}",
                before.0, after.0, before.1, after.1, before.2, after.2
            );
        }
    }
    pub(crate) fn reset(&mut self) {
        self.slots.clear();
        self.marks.clear();
        self.free.clear();
        self.retired_slots = 0;
        self.generations.clear();
        self.external_bytes = 0;
        self.allocations = 0;
        self.threshold = 384;
        self.total_allocations = 0;
        self.collections = 0;
        self.peak_live = 0;
        self.peak_survivors = 0;
        self.max_threshold = 384;
        self.properties.reset();
        self.roots.clear();
        self.sparse_arrays = None;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = GcProfile::default();
        }
        #[cfg(feature = "profile-memory")]
        {
            self.memory_profile = memory_profile::MemoryProfile::default();
        }
    }
    pub fn should_collect(&self) -> bool {
        self.allocations >= self.threshold
    }
    pub(crate) fn root(&mut self, value: Value) -> RootId {
        self.roots.insert(value)
    }
    pub(crate) fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.roots.update(root, value)
    }
    pub(crate) fn release_root(&mut self, root: RootId) -> bool {
        self.roots.remove(root)
    }
    #[cfg(test)]
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
        self.collect_with_shape_roots(roots, |_, _| {})
    }
    pub(crate) fn collect_with_shape_roots(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        mut shape_roots: impl FnMut(u32, &mut Vec<Value>),
    ) -> Vec<(Value, Value)> {
        self.collections += 1;
        #[cfg(feature = "profile-aggregate")]
        let mark_started = std::time::Instant::now();
        let mut work: Vec<Value> = roots.into_iter().chain(self.roots.values()).collect();
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.roots += work.len() as u64;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
        }
        self.mark_work(&mut work, &mut shape_roots);
        self.mark_ephemerons(&mut work, &mut shape_roots);
        let finalization_jobs = self.prune_weak_entries();
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.mark_nanos += mark_started.elapsed().as_nanos() as u64;
        }
        #[cfg(feature = "profile-aggregate")]
        let sweep_started = std::time::Instant::now();
        let mut live = 0;
        let properties = &mut self.properties;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += self.slots.len() as u64;
        }
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if Self::marked(&self.marks, index) {
                live += 1;
            } else if let Some(cell) = slot.cell.take() {
                self.external_bytes = self.external_bytes.saturating_sub(cell.external_bytes());
                #[cfg(feature = "profile-memory")]
                self.memory_profile.freed(index, &cell);
                if let Some(object) = cell.object() {
                    properties.release(object.properties);
                }
                if let Some(arrays) = &mut self.sparse_arrays {
                    arrays.remove(&(index as u32));
                }
                self.free.push(index as u32);
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.freed += 1;
                }
            }
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_nanos += sweep_started.elapsed().as_nanos() as u64;
        }
        self.marks.fill(0);
        self.allocations = 0;
        // `threshold` counts allocations *after* this collection. Half the
        // surviving set therefore targets a 1.5x total occupied high-water.
        let headroom = if live >= 1 << 16 { live } else { live / 2 };
        self.threshold = headroom.max(384);
        self.peak_survivors = self.peak_survivors.max(live);
        self.max_threshold = self.max_threshold.max(self.threshold);
        finalization_jobs
    }
    pub(super) fn mark_work(
        &mut self,
        work: &mut Vec<Value>,
        shape_roots: &mut impl FnMut(u32, &mut Vec<Value>),
    ) {
        while let Some(value) = work.pop() {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.work_items += 1;
            }
            let Some(index) = value.heap_index().map(|value| value as usize) else {
                continue;
            };
            let Some(slot) = self.slots.get_mut(index) else {
                continue;
            };
            if Self::marked(&self.marks, index) || slot.cell.is_none() {
                continue;
            }
            Self::mark(&mut self.marks, index);
            let cell = slot.cell.as_ref().unwrap();
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[Self::cell_kind(cell) as usize] += 1;
            }
            Self::children(cell, &self.properties, work, shape_roots);
            if let Some(elements) = self
                .sparse_arrays
                .as_ref()
                .and_then(|arrays| arrays.get(&(index as u32)))
            {
                work.extend(elements.values.values().copied());
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
            }
        }
    }
    #[inline(always)]
    fn marked(marks: &[u64], index: usize) -> bool {
        marks[index / 64] & (1 << (index % 64)) != 0
    }
    #[inline(always)]
    fn mark(marks: &mut [u64], index: usize) {
        marks[index / 64] |= 1 << (index % 64);
    }
    #[cfg(feature = "profile-aggregate")]
    pub(crate) const fn gc_profile(&self) -> GcProfile {
        self.gc_profile
    }
    pub(crate) fn sparse_get(&self, array: Value, index: usize) -> Option<Value> {
        self.sparse_arrays
            .as_ref()?
            .get(&array.heap_index()?)?
            .values
            .get(&index)
            .copied()
    }
    pub(crate) fn sparse_length(&self, array: Value) -> Option<usize> {
        self.sparse_arrays
            .as_ref()?
            .get(&array.heap_index()?)
            .map(|elements| elements.length)
    }
    pub(crate) fn sparse_set(&mut self, array: Value, index: usize, value: Value) {
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array.heap_index().unwrap()).or_default();
        elements.values.insert(index, value);
        elements.length = elements.length.max(index.saturating_add(1));
    }
    pub(crate) fn sparse_set_length(&mut self, array: Value, length: usize) {
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array.heap_index().unwrap()).or_default();
        elements.values.retain(|index, _| *index < length);
        elements.length = length;
    }
    pub(crate) fn property_get(&self, object: &Object, slot: usize) -> Option<Value> {
        self.properties
            .get(object.properties, slot)
            .filter(|value| !value.is_deleted())
    }
    pub(crate) unsafe fn property_get_unchecked(&self, object: &Object, slot: usize) -> Value {
        unsafe { self.properties.get_unchecked(object.properties, slot) }
    }
    pub(crate) fn property_set(&mut self, object: Value, slot: usize, value: Value) {
        let vector = self.get(object).unwrap().object().unwrap().properties;
        self.properties.set(vector, slot, value);
    }
    pub(crate) unsafe fn property_set_unchecked(
        &mut self,
        object: Value,
        slot: usize,
        value: Value,
    ) {
        let vector = self.get(object).unwrap().object().unwrap().properties;
        unsafe { self.properties.set_unchecked(vector, slot, value) };
    }
    pub(crate) fn property_push(&mut self, object: Value, value: Value) {
        let mut vector = self.get(object).unwrap().object().unwrap().properties;
        self.properties.push(&mut vector, value);
        self.get_mut(object)
            .unwrap()
            .object_mut()
            .unwrap()
            .properties = vector;
    }
    fn children(
        cell: &Cell,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        shape_roots: &mut impl FnMut(u32, &mut Vec<Value>),
    ) {
        let mut object = |object: &Object| {
            work.push(object.proto);
            shape_roots(object.shape(), work);
            work.extend(object.private_names().iter().map(|brand| brand.home));
            properties.append_live_values(object.properties, work);
        };
        if let Some((value, buffer)) = cell.typed_array_backing() {
            object(value);
            work.push(buffer);
            return;
        }
        match cell {
            Cell::Object(value) => object(value),
            Cell::Array {
                object: value,
                elements,
            } => {
                object(value);
                work.extend(elements.iter().copied());
            }
            Cell::ArrayBuffer { object: value, .. } => object(value),
            Cell::RegExp { object: value, .. } => object(value),
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
                work.extend(entries.iter().flat_map(|(key, value)| [*key, *value]));
            }
            Cell::Set {
                object: value,
                entries,
            } => {
                object(value);
                work.extend(entries.iter().copied());
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
            Cell::Iterator {
                object: value,
                source,
                next_method,
                helper,
                generator,
                ..
            } => {
                object(value);
                work.push(*source);
                work.extend(*next_method);
                if let Some(helper) = helper {
                    match helper.as_ref() {
                        IteratorHelper::Map { callback, .. }
                        | IteratorHelper::Filter { callback, .. }
                        | IteratorHelper::FlatMap { callback, .. } => work.push(*callback),
                        IteratorHelper::Take { .. }
                        | IteratorHelper::Drop { .. }
                        | IteratorHelper::RegExpStringMatchAll { .. } => {}
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
                    if let Some(continuation) = record.continuation.as_ref() {
                        work.extend(continuation.roots());
                    }
                    work.extend(
                        record
                            .requests
                            .iter()
                            .flat_map(|request| [request.promise, request.value]),
                    );
                }
            }
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
            } => {
                object(value);
                work.extend([*target, *handler]);
            }
            Cell::Function {
                object: value,
                env,
                realm,
                ..
            } => {
                object(value);
                work.extend([*env, *realm]);
            }
            Cell::Environment {
                parent,
                slots,
                dynamic_bindings,
                with_objects,
                ..
            } => {
                work.push(*parent);
                work.extend(slots.iter().copied());
                work.extend(dynamic_bindings.iter().map(|(_, value)| *value));
                work.extend(with_objects.iter().copied());
            }
            Cell::PromiseResolvingState { promise, .. } => work.push(*promise),
            Cell::Date { object: value, .. }
            | Cell::TemporalDuration { object: value, .. }
            | Cell::TemporalPlainDate { object: value, .. }
            | Cell::TemporalPlainDateTime { object: value, .. }
            | Cell::TemporalPlainMonthDay { object: value, .. }
            | Cell::TemporalPlainYearMonth { object: value, .. }
            | Cell::TemporalZonedDateTime { object: value, .. }
            | Cell::TemporalInstant { object: value, .. } => object(value),
            Cell::String(_)
            | Cell::BigInt(_)
            | Cell::Symbol(_)
            | Cell::Error(_)
            | Cell::WasmBits64(_) => {}
            _ => unreachable!("typed array backing handled above"),
        }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn cell_kind(cell: &Cell) -> CellKind {
        match cell {
            Cell::Object(_)
            | Cell::ArrayBuffer { .. }
            | Cell::TypedArray { .. }
            | Cell::DataView { .. }
            | Cell::ShadowRealm { .. }
            | Cell::Proxy { .. }
            | Cell::PromiseResolvingState { .. } => CellKind::Object,
            Cell::Array { .. } => CellKind::Array,
            Cell::Map { .. } => CellKind::Map,
            Cell::Set { .. } => CellKind::Set,
            Cell::Iterator { .. } => CellKind::Iterator,
            Cell::WeakMap { .. } => CellKind::WeakMap,
            Cell::WeakSet { .. } => CellKind::WeakSet,
            Cell::WeakRef { .. } | Cell::FinalizationRegistry { .. } => CellKind::WeakRef,
            Cell::Function { .. } => CellKind::Function,
            Cell::Environment { .. } => CellKind::Environment,
            Cell::String(_) => CellKind::String,
            Cell::BigInt(_) => CellKind::BigInt,
            Cell::WasmBits64(_) => CellKind::WasmBits64,
            Cell::Symbol(_) => CellKind::Symbol,
            Cell::Date { .. } => CellKind::Date,
            Cell::Error(_) => CellKind::Error,
            Cell::RegExp { .. } => CellKind::RegExp,
            Cell::ArrayFromAsyncState(_) => CellKind::ArrayFromAsync,
            Cell::TemporalDuration { .. } => CellKind::TemporalDuration,
            Cell::TemporalPlainDate { .. } => CellKind::TemporalPlainDate,
            Cell::TemporalPlainDateTime { .. } => CellKind::TemporalPlainDateTime,
            Cell::TemporalPlainMonthDay { .. } => CellKind::TemporalPlainMonthDay,
            Cell::TemporalPlainYearMonth { .. } => CellKind::TemporalPlainYearMonth,
            Cell::TemporalZonedDateTime { .. } => CellKind::TemporalZonedDateTime,
            Cell::TemporalInstant { .. } => CellKind::TemporalInstant,
        }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn cell_payload_bytes(cell: &Cell) -> usize {
        let object_extra_bytes = cell
            .object()
            .map_or(0, |object| object.allocated_extra_bytes());
        object_extra_bytes
            + match cell {
                Cell::Object(_)
                | Cell::ShadowRealm { .. }
                | Cell::Iterator { .. }
                | Cell::ArrayFromAsyncState(_)
                | Cell::Proxy { .. }
                | Cell::Date { .. }
                | Cell::PromiseResolvingState { .. }
                | Cell::TemporalDuration { .. }
                | Cell::TemporalInstant { .. }
                | Cell::TypedArray { .. }
                | Cell::DataView { .. }
                | Cell::WeakRef { .. }
                | Cell::FinalizationRegistry { .. }
                | Cell::WasmBits64(_) => 0,
                Cell::TemporalPlainDate { calendar, .. }
                | Cell::TemporalPlainDateTime { calendar, .. }
                | Cell::TemporalPlainMonthDay { calendar, .. }
                | Cell::TemporalPlainYearMonth { calendar, .. } => calendar.capacity(),
                Cell::TemporalZonedDateTime {
                    time_zone,
                    calendar,
                    ..
                } => time_zone.capacity() + calendar.capacity(),
                Cell::RegExp { source, flags, .. } => source.capacity() + flags.capacity(),
                Cell::Array { elements, .. } => elements.capacity() * size_of::<Value>(),
                Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
                Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
                Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::WeakMap { entries, .. } => entries.allocated_bytes(),
                Cell::WeakSet { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::Function { .. } => size_of::<Object>(),
                Cell::Environment {
                    slots,
                    with_objects,
                    ..
                } => (slots.len() + with_objects.capacity()) * size_of::<Value>(),
                Cell::String(value) => value.capacity(),
                Cell::BigInt(value) | Cell::Error(value) => value.capacity(),
                Cell::Symbol(value) => value.as_ref().map_or(0, String::capacity),
            }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn size_bucket(bytes: usize) -> usize {
        match bytes {
            0 => 0,
            1..=7 => 1,
            8..=15 => 2,
            16..=31 => 3,
            32..=63 => 4,
            64..=127 => 5,
            128..=255 => 6,
            _ => 7,
        }
    }
}
#[cfg(test)]
mod tests;
