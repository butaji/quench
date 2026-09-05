//! Periodic trial-deletion collection for `Rc` cycles.
//!
//! `Rc` remains the primary ownership mechanism.  This module only tracks
//! identity-bearing objects that participate in mutable property/capture
//! edges, and runs a QuickJS-shaped three-phase pass after an allocation-byte
//! budget is exhausted.  The registry is weak, so it cannot keep otherwise
//! unreachable values alive.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
};

use crate::value::{FunctionValue, ObjectData, PrivateSlot, Value};

// Keep the trigger in the same order of magnitude as QuickJS's allocation
// budget while allowing for Rust's larger per-node metadata.  The threshold
// grows from this floor based on the surviving graph after each pass.
const INITIAL_THRESHOLD: usize = 512 * 1024;

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static ROOTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static ENV_ROOTS: RefCell<Vec<Rc<crate::environment::Environment>>> = const { RefCell::new(Vec::new()) };
    static FRAME_ROOTS: RefCell<Vec<FrameRoot>> = const { RefCell::new(Vec::new()) };
}

struct FrameRoot {
    registers: *const crate::register_file::RegisterFile,
    environment: Rc<crate::environment::Environment>,
}

/// Keep a call continuation's Rust-owned values visible to trial deletion.
/// Continuations are temporary host-side roots while a callee executes; they
/// are not necessarily reachable from the callee's lexical object graph.
pub(crate) struct RootGuard {
    value_length: usize,
    environment_length: usize,
    frame_length: usize,
}

pub(crate) fn protect_call(continuation: &crate::completion::CallContinuation) -> RootGuard {
    ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        let value_length = roots.len();
        roots.push(continuation.callee.clone());
        roots.push(continuation.receiver.clone());
        roots.extend(continuation.arguments.iter().cloned());
        continuation
            .caller_registers
            .visit_values(|value| roots.push(value));
        let environment_length = ENV_ROOTS.with(|environments| environments.borrow().len());
        RootGuard {
            value_length,
            environment_length,
            frame_length: FRAME_ROOTS.with(|frames| frames.borrow().len()),
        }
    })
}

/// Keep a live residual frame visible while a helper-capable native bridge
/// runs. The collector scans the packed words only when collection occurs;
/// entry/exit therefore does not decode or clone every register.
pub(crate) fn protect_frame(
    registers: &crate::register_file::RegisterFile,
    environment: &Rc<crate::environment::Environment>,
) -> RootGuard {
    let value_length = ROOTS.with(|roots| roots.borrow().len());
    let frame_length = FRAME_ROOTS.with(|frames| {
        let mut frames = frames.borrow_mut();
        let length = frames.len();
        frames.push(FrameRoot {
            registers,
            environment: Rc::clone(environment),
        });
        length
    });
    let environment_length = ENV_ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        let length = roots.len();
        roots.push(Rc::clone(environment));
        length
    });
    RootGuard {
        value_length,
        environment_length,
        frame_length,
    }
}

/// Retain a nested active callee in the same root set. The packed call-frame
/// driver advances nested calls iteratively, so their `FunctionValue`s live in
/// Rust-owned `ActiveCall` records rather than in the JS graph.
pub(crate) fn retain_active_function(value: &Value) {
    ROOTS.with(|roots| roots.borrow_mut().push(value.clone()));
}

pub(crate) fn retain_active_environment(environment: &Rc<crate::environment::Environment>) {
    ENV_ROOTS.with(|roots| roots.borrow_mut().push(Rc::clone(environment)));
}

pub(crate) fn protect_environment(environment: &Rc<crate::environment::Environment>) -> RootGuard {
    let value_length = ROOTS.with(|roots| roots.borrow().len());
    let environment_length = ENV_ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        let length = roots.len();
        roots.push(Rc::clone(environment));
        length
    });
    RootGuard {
        value_length,
        environment_length,
        frame_length: FRAME_ROOTS.with(|frames| frames.borrow().len()),
    }
}

impl Drop for RootGuard {
    fn drop(&mut self) {
        ROOTS.with(|roots| roots.borrow_mut().truncate(self.value_length));
        ENV_ROOTS.with(|roots| roots.borrow_mut().truncate(self.environment_length));
        FRAME_ROOTS.with(|frames| frames.borrow_mut().truncate(self.frame_length));
    }
}

#[derive(Default)]
struct State {
    objects: HashMap<usize, Weak<ObjectData>>,
    functions: HashMap<usize, Weak<FunctionValue>>,
    bytes_since_gc: usize,
    threshold: usize,
    collecting: bool,
}

enum Node {
    Object(Rc<ObjectData>),
    Function(Rc<FunctionValue>),
}

impl Node {
    fn key(&self) -> usize {
        match self {
            Self::Object(value) => Rc::as_ptr(value) as usize,
            Self::Function(value) => Rc::as_ptr(value) as usize,
        }
    }
}

/// Record a property receiver in the weak live-object registry.
pub(crate) fn track_object(value: &Rc<ObjectData>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let key = Rc::as_ptr(value) as usize;
        let needs_insert = state
            .objects
            .get(&key)
            .is_none_or(|entry| entry.strong_count() == 0);
        if needs_insert {
            state.objects.insert(key, Rc::downgrade(value));
            state.bytes_since_gc = state.bytes_since_gc.saturating_add(256);
        }
    });
}

/// Record a function whose capture/property edges may participate in a cycle.
pub(crate) fn track_function(value: &Rc<FunctionValue>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let key = Rc::as_ptr(value) as usize;
        let needs_insert = state
            .functions
            .get(&key)
            .is_none_or(|entry| entry.strong_count() == 0);
        if needs_insert {
            state.functions.insert(key, Rc::downgrade(value));
            state.bytes_since_gc = state.bytes_since_gc.saturating_add(256);
        }
    });
}

/// Track a heap value encountered at a mutable property edge.
pub(crate) fn track_value(value: &Value) {
    match value {
        Value::Object(object) => track_object(object),
        Value::Function(function) => track_function(function),
        Value::WeakFunction(function) => {
            if let Some(function) = function.0.upgrade() {
                track_function(&function);
            }
        }
        _ => {}
    }
}

/// Amortized checkpoint.  The full live registry is inspected only after the
/// allocation-byte budget is exhausted, never on every ordinary read/write.
pub(crate) fn checkpoint() {
    let should_collect = STATE.with(|state| {
        let state = state.borrow();
        !state.collecting && state.bytes_since_gc >= state.threshold.max(INITIAL_THRESHOLD)
    });
    if should_collect {
        collect_cycles();
    }
}

/// Run one complete trial-deletion pass.  This is also exposed to tests and
/// host integration as a deterministic checkpoint.
pub(crate) fn collect_cycles() {
    // Ensure the active global is represented as a graph node before taking
    // the registry snapshot.  Its outgoing properties are then handled by
    // the normal graph walk, avoiding a recursive root traversal on every
    // allocation checkpoint.
    if let Value::Object(global) = crate::vm::current_global_object() {
        track_object(&global);
    }
    let (objects, functions) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.collecting {
            return (Vec::new(), Vec::new());
        }
        state.collecting = true;
        state.bytes_since_gc = 0;
        (
            state
                .objects
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>(),
            state
                .functions
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>(),
        )
    });

    let mut nodes = Vec::with_capacity(objects.len() + functions.len());
    nodes.extend(objects.into_iter().map(Node::Object));
    nodes.extend(functions.into_iter().map(Node::Function));
    let mut ids = HashMap::with_capacity(nodes.len());
    nodes.retain(|node| ids.insert(node.key(), ids.len()).is_none());
    let mut edges = vec![Vec::new(); nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        match node {
            Node::Object(object) => {
                for (_, value) in object.iter() {
                    append_edges(&value, &ids, &mut edges[index]);
                }
                if let Some(value) = object.original_prototype() {
                    append_edges(&value, &ids, &mut edges[index]);
                }
                if let Some(replacement) = object.replacement() {
                    append_edges(&Value::Object(replacement), &ids, &mut edges[index]);
                }
            }
            Node::Function(function) => {
                for value in function.cycle_values() {
                    append_edges(&value, &ids, &mut edges[index]);
                }
            }
        }
    }

    // Phase 1/2: subtract internal edges, then restore edges reachable from
    // externally-owned nodes.  The remaining zero nodes are garbage cycles.
    let mut incoming = vec![0usize; nodes.len()];
    for outgoing in &edges {
        for &target in outgoing {
            incoming[target] = incoming[target].saturating_add(1);
        }
    }
    let mut external = vec![false; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        let strong = match node {
            Node::Object(value) => Rc::strong_count(value),
            Node::Function(value) => Rc::strong_count(value),
        };
        // `nodes` owns one temporary reference to every candidate.
        external[index] = strong.saturating_sub(1) > incoming[index];
    }
    // Include temporary Rust-side roots retained by the VM call driver. Mark
    // the corresponding graph nodes external, then the normal reachability
    // walk below preserves everything they reference.
    ROOTS.with(|roots| {
        for value in roots.borrow().iter() {
            match value {
                Value::Object(object) => {
                    if let Some(&id) = ids.get(&(Rc::as_ptr(object) as usize)) {
                        external[id] = true;
                    }
                }
                Value::Function(function) => {
                    if let Some(&id) = ids.get(&(Rc::as_ptr(function) as usize)) {
                        external[id] = true;
                    }
                }
                Value::BindingCell(cell) => match cell.load() {
                    Value::Object(object) => {
                        if let Some(&id) = ids.get(&(Rc::as_ptr(&object) as usize)) {
                            external[id] = true;
                        }
                    }
                    Value::Function(function) => {
                        if let Some(&id) = ids.get(&(Rc::as_ptr(&function) as usize)) {
                            external[id] = true;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    });
    ENV_ROOTS.with(|environments| {
        for environment in environments.borrow().iter() {
            for value in environment.cycle_values() {
                match value {
                    Value::Object(object) => {
                        if let Some(&id) = ids.get(&(Rc::as_ptr(&object) as usize)) {
                            external[id] = true;
                        }
                    }
                    Value::Function(function) => {
                        if let Some(&id) = ids.get(&(Rc::as_ptr(&function) as usize)) {
                            external[id] = true;
                        }
                    }
                    Value::BindingCell(cell) => match cell.load() {
                        Value::Object(object) => {
                            if let Some(&id) = ids.get(&(Rc::as_ptr(&object) as usize)) {
                                external[id] = true;
                            }
                        }
                        Value::Function(function) => {
                            if let Some(&id) = ids.get(&(Rc::as_ptr(&function) as usize)) {
                                external[id] = true;
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    });
    FRAME_ROOTS.with(|frames| {
        for frame in frames.borrow().iter() {
            // SAFETY: a FrameRoot is installed only while the synchronous
            // caller owns the RegisterFile; RootGuard removes it before that
            // owner can leave or be relocated.
            unsafe { (&*frame.registers).visit_values(|value| {
                mark_direct_root_value(&value, &ids, &mut external);
            }) };
            for value in frame.environment.cycle_values() {
                mark_direct_root_value(&value, &ids, &mut external);
            }
        }
    });
    // The global object is a host/runtime root even when no VM call frame is
    // active (for example, between repeated benchmark runs). Marking its
    // registry node lets the normal graph walk preserve globally reachable
    // closures and their captured values. The global itself was not always in
    // the weak registry, hence the explicit admission above.
    mark_direct_root_value(&crate::vm::current_global_object(), &ids, &mut external);
    if let Some(global_lexical) = crate::locals::global_lexical() {
        for value in global_lexical.cycle_values() {
            mark_direct_root_value(&value, &ids, &mut external);
        }
    }
    let mut reachable = external.clone();
    let mut work = external
        .iter()
        .enumerate()
        .filter_map(|(i, live)| live.then_some(i))
        .collect::<Vec<_>>();
    while let Some(index) = work.pop() {
        for &target in &edges[index] {
            if !reachable[target] {
                reachable[target] = true;
                work.push(target);
            }
        }
    }
    let doomed = reachable
        .iter()
        .enumerate()
        .filter_map(|(i, live)| (!live).then_some(i))
        .collect::<HashSet<_>>();
    if !doomed.is_empty() {
        for (index, node) in nodes.iter().enumerate() {
            if !doomed.contains(&index) {
                continue;
            }
            match node {
                Node::Object(object) => clear_object_edges(object, &doomed, &ids),
                Node::Function(function) => clear_function_edges(function, &doomed, &ids),
            }
        }
    }

    let live = nodes.len().saturating_sub(doomed.len());
    // Release the collector's temporary ownership before pruning the weak
    // registry; otherwise every candidate appears spuriously live.
    drop(nodes);
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.objects.retain(|_, entry| entry.strong_count() > 0);
        state.functions.retain(|_, entry| entry.strong_count() > 0);
        // QuickJS adapts its next pass to the surviving allocation volume.
        // Keep a floor so small programs do not turn collection into a tax.
        state.threshold = (live.max(1) * 256).max(INITIAL_THRESHOLD);
        state.collecting = false;
    });
}

fn append_edges(value: &Value, ids: &HashMap<usize, usize>, output: &mut Vec<usize>) {
    match value {
        Value::Object(object) => {
            if let Some(&id) = ids.get(&(Rc::as_ptr(object) as usize)) {
                output.push(id);
            }
        }
        Value::Function(function) => {
            if let Some(&id) = ids.get(&(Rc::as_ptr(function) as usize)) {
                output.push(id);
            }
        }
        Value::WeakFunction(function) => {
            // WeakFunction is deliberately non-owning.  It may resolve while
            // the target is live, but it must not keep a trial-deleted node
            // alive or count as an internal strong edge.
            let _ = function;
            return;
        }
        Value::BindingCell(cell) => append_edges(&cell.load(), ids, output),
        Value::Proxy(proxy) => {
            append_edges(&proxy.target, ids, output);
            append_edges(&proxy.handler, ids, output);
        }
        Value::BoundFunction(function) => {
            append_edges(&function.target, ids, output);
            append_edges(&function.receiver, ids, output);
            for value in &function.arguments {
                append_edges(value, ids, output);
            }
            for (_, value) in function.properties.borrow().iter() {
                append_edges(value, ids, output);
            }
        }
        _ => {}
    }
}

fn mark_direct_root_value(value: &Value, ids: &HashMap<usize, usize>, external: &mut [bool]) {
    fn visit(
        value: &Value,
        ids: &HashMap<usize, usize>,
        external: &mut [bool],
        seen: &mut HashSet<usize>,
    ) {
        match value {
            Value::Object(object) => {
                let key = Rc::as_ptr(object) as usize;
                if !seen.insert(key) {
                    return;
                }
                if let Some(&id) = ids.get(&key) {
                    external[id] = true;
                }
                // Outgoing edges are traversed by the ordinary reachability
                // walk once this root node is marked.
            }
            Value::Function(function) => {
                let key = Rc::as_ptr(function) as usize;
                if !seen.insert(key) {
                    return;
                }
                if let Some(&id) = ids.get(&key) {
                    external[id] = true;
                }
                // As above, the graph walk handles the function's edges.
            }
            Value::BindingCell(_)
            | Value::ObjectAlias(_)
            | Value::Proxy(_)
            | Value::BoundFunction(_) => {}
            Value::WeakFunction(_)
            | Value::HostCapability(_)
            | Value::Builtin(_)
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::BigInt(_)
            | Value::Array(_)
            | Value::ArrayBuffer(_)
            | Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
            | Value::DataView(_)
            | Value::Promise(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Iterator(_)
            | Value::Generator(_)
            | Value::Number(_)
            | Value::Boolean(_)
            | Value::Null
            | Value::Undefined => {}
        }
    }
    visit(value, ids, external, &mut HashSet::new());
}

pub(crate) fn value_points_to_doomed(
    value: &Value,
    doomed: &HashSet<usize>,
    ids: &HashMap<usize, usize>,
) -> bool {
    match value {
        Value::Object(object) => ids
            .get(&(Rc::as_ptr(object) as usize))
            .is_some_and(|id| doomed.contains(id)),
        Value::Function(function) => ids
            .get(&(Rc::as_ptr(function) as usize))
            .is_some_and(|id| doomed.contains(id)),
        // Weak references are not cleared by trial deletion: once their
        // target drops, `WeakFunctionValue::value` already resolves to
        // `Undefined`, and they never own the target in the first place.
        Value::WeakFunction(_) => false,
        Value::BindingCell(cell) => value_points_to_doomed(&cell.load(), doomed, ids),
        _ => false,
    }
}

fn clear_object_edges(
    object: &Rc<ObjectData>,
    doomed: &HashSet<usize>,
    ids: &HashMap<usize, usize>,
) {
    for slot in 0..object.len() {
        if let Some(value) = object.slot_value(slot) {
            if value_points_to_doomed(&value, doomed, ids) {
                if let Some(word) = object.slot_word(slot) {
                    word.store(Value::Undefined);
                }
            }
        }
    }
    if object
        .original_prototype()
        .is_some_and(|value| value_points_to_doomed(&value, doomed, ids))
    {
        object.clear_original_prototype();
    }
    if object.replacement().is_some_and(|value| {
        doomed
            .iter()
            .any(|id| ids.get(&(Rc::as_ptr(&value) as usize)) == Some(id))
    }) {
        object.clear_replacement();
    }
}

fn clear_function_edges(
    function: &Rc<FunctionValue>,
    doomed: &HashSet<usize>,
    ids: &HashMap<usize, usize>,
) {
    {
        let mut properties = function.properties.borrow_mut();
        for (_, value) in properties.iter_mut() {
            if value_points_to_doomed(value, doomed, ids) {
                *value = Value::Undefined;
            }
        }
    }
    // `with_captures` is immutable by design.  It is not used for lexical
    // variable storage; the mutable `Environment` capture above is the edge
    // that can participate in ordinary closure cycles.
    function.captures.clear_cycle_edges(doomed, ids);
    let mut private_slots = function.private_slots.borrow_mut();
    for (_, slot) in private_slots.iter_mut() {
        match slot {
            PrivateSlot::Data(value) | PrivateSlot::Method(value) => {
                if value_points_to_doomed(value, doomed, ids) {
                    *value = Value::Undefined;
                }
            }
            PrivateSlot::Accessor { get, set } => {
                if get
                    .as_ref()
                    .is_some_and(|value| value_points_to_doomed(value, doomed, ids))
                {
                    *get = None;
                }
                if set
                    .as_ref()
                    .is_some_and(|value| value_points_to_doomed(value, doomed, ids))
                {
                    *set = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::Environment;
    use crate::value::ObjectData;

    #[test]
    fn trial_deletion_breaks_unreachable_object_cycle() {
        let left = Rc::new(ObjectData::new(vec![("peer".into(), Value::Undefined)]));
        let right = Rc::new(ObjectData::new(vec![("peer".into(), Value::Undefined)]));
        track_object(&left);
        track_object(&right);
        // Build the cycle before introducing any additional owners.
        left.slot_word(0)
            .unwrap()
            .store(Value::Object(Rc::clone(&right)));
        right
            .slot_word(0)
            .unwrap()
            .store(Value::Object(Rc::clone(&left)));
        let left_weak = Rc::downgrade(&left);
        let right_weak = Rc::downgrade(&right);
        drop(left);
        drop(right);
        collect_cycles();
        assert!(left_weak.upgrade().is_none());
        assert!(right_weak.upgrade().is_none());
    }

    #[test]
    fn closure_environment_values_are_visible_to_the_pass() {
        let environment = Environment::new();
        let object = Rc::new(ObjectData::new(Vec::new()));
        environment.set(0, Value::Object(Rc::clone(&object)));
        let values = environment.cycle_values();
        assert!(values.iter().any(
            |value| matches!(value, Value::Object(candidate) if Rc::ptr_eq(candidate, &object))
        ));
    }

    #[test]
    fn protected_frame_roots_live_register_words_until_bridge_exit() {
        let object = Rc::new(ObjectData::new(Vec::new()));
        track_object(&object);
        let weak = Rc::downgrade(&object);
        let registers = crate::register_file::RegisterFile::from_values(vec![
            Value::Object(Rc::clone(&object)),
        ]);
        let environment = Environment::new();
        let guard = protect_frame(&registers, &environment);
        drop(object);
        collect_cycles();
        assert!(weak.upgrade().is_some(), "frame root was collected during bridge");
        drop(guard);
        drop(registers);
        collect_cycles();
        assert!(weak.upgrade().is_none(), "temporary frame root leaked past bridge exit");
    }
}
