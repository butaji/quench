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
}
