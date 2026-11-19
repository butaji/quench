use super::CompilerAtoms;

use crate::bytecode::{Function, Op, Traits, WITH_ENTER_BINDING, the_op, may_repeat};

const ENV_LOCAL_LOAD_OP: Op = the_op(Traits::ENV_LOCAL_LOAD);
const ENV_LOCAL_STORE_OP: Op = the_op(Traits::ENV_LOCAL_STORE);
const NOP_OP: Op = the_op(Traits::NO_OP);

/// What the layout of a function's activation depends on beyond its own code: the slots of
/// its locals that functions nested in it capture, and whether one of them resolves names
/// at run time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayoutInputs {
    pub(crate) captured: Vec<u16>,
    pub(crate) dynamic_descendant: bool,
    /// Whether the function is a program root that splits like a function: a module's body,
    /// whose module-scope bindings importers and the linker address by slot, or a Script's or
    /// eval's root without dynamic name resolution, whose global declarations the realm and later
    /// code find by slot. Those bindings are in `captured`; a Script's top-level lexicals are realm
    /// bindings with no root slot, so they are not. Its block bindings, such as
    /// a `for (let ...)` loop's, are as private to its activation as a function's.
    pub(crate) split_root: bool,
    /// The depth-0 captures of the parent's never-written declarations (`Function::declared_captures`).
    pub(crate) declared_captures: Vec<u16>,
}

/// What the program-wide layout analyses read of one function, fixed once it is compiled:
/// a function nested in it may be finished and its record dropped before the program is.
#[derive(Clone, Debug, Default)]
pub(crate) struct FunctionFacts {
    pub(crate) parent: Option<u32>,
    pub(crate) is_arrow: bool,
    pub(crate) locals: u16,
    /// `Function::has_dynamic_scope_access`.
    pub(crate) dynamic_access: bool,
    /// `Function::contains_with_statement`.
    pub(crate) contains_with: bool,
    /// Whether the function has binding sites, which its nested functions resolve through.
    pub(crate) binding_sites: bool,
    /// The (distance, slot) of each capture: the slot of the function `distance` levels out.
    pub(crate) captures: Box<[(u32, u16)]>,
    /// The slots of its bindings that the runtime finds by name from outside its activation
    /// (`local_name_read_by_name`).
    pub(crate) named_slots: Box<[u16]>,
    /// `Function::declared_functions`.
    pub(crate) declared: Box<[u16]>,
    /// The (distance, slot) of each store to a local (distance 0: the function's own) or a capture (the function
    /// `distance` levels out), for every op that writes a local slot or a capture.
    pub(crate) stores: Box<[(u32, u16)]>,
}

impl FunctionFacts {
    pub(super) fn of(function: &Function, atoms: &CompilerAtoms<'_>) -> Self {
        let is_with_enter = |atom: usize| {
            atoms
                .get(atom)
                .is_some_and(|name| name == WITH_ENTER_BINDING)
        };
        let mut captures: Vec<(u32, u16)> = function
            .code
            .iter()
            .filter(|instruction| instruction.op().addresses_capture())
            .map(|instruction| (instruction.capture_depth(), instruction.capture_slot()))
            .chain(
                function
                    .wide
                    .iter()
                    .filter(|instruction| instruction.op().addresses_capture())
                    .map(|instruction| (instruction.capture_depth(), instruction.capture_slot())),
            )
            .map(|(depth, slot)| (u32::from(depth) + 1, slot))
            .collect();
        captures.sort_unstable();
        captures.dedup();
        // `code` is the function's instructions (`Function::instructions` widens each); `wide` is an expansion of it.
        let stores: Vec<(u32, u16)> = function.code.iter()
            .filter_map(|instruction| {
                if writes_local(instruction.op()) {
                    Some((0, u16::try_from(instruction.local_slot()).ok()?))
                } else if instruction.op() == Op::StoreCapture {
                    Some((u32::from(instruction.capture_depth()) + 1, instruction.capture_slot()))
                } else {
                    None
                }
            })
            .collect();
        Self {
            parent: function.parent,
            is_arrow: function.is_arrow,
            locals: function.locals,
            dynamic_access: function.has_dynamic_scope_access(),
            contains_with: function.contains_with_statement(is_with_enter),
            binding_sites: !function.binding_sites.is_empty(),
            captures: captures.into(),
            named_slots: slots_read_by_name(function, atoms).collect(),
            declared: function.declared_functions.iter().copied().collect(),
            stores: stores.into(),
        }
    }
}

/// Whether `op` stores to a local slot: an op whose immediate names one and that does not load it.
fn writes_local(op: Op) -> bool {
    op.immediate_role() == crate::bytecode::ImmediateRole::LocalSlot
        && !matches!(op, Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal)
}

/// The slots of `function`'s compiler bindings, which the runtime finds by name in its
/// environment, so they live there whether or not a closure captures them.
fn named_environment_slots<'f>(function: &'f Function, atoms: &'f CompilerAtoms<'_>) -> impl Iterator<Item = u16> + 'f {
    function
        .local_atoms
        .iter()
        .enumerate()
        .filter(|(_, atom)| {
            atoms
                .get(**atom as usize)
                .is_some_and(crate::bytecode::local_name_requires_environment)
        })
        .filter_map(|(slot, _)| u16::try_from(slot).ok())
}

/// The slots of `function`'s bindings that the runtime finds by name from outside its
/// activation (`local_name_read_by_name`).
fn slots_read_by_name<'f>(function: &'f Function, atoms: &'f CompilerAtoms<'_>) -> impl Iterator<Item = u16> + 'f {
    function
        .local_atoms
        .iter()
        .enumerate()
        .filter(|(_, atom)| {
            atoms.get(**atom as usize).is_some_and(|name| {
                crate::bytecode::local_name_read_by_name(name, || function.environment_atoms.contains(*atom))
            })
        })
        .filter_map(|(slot, _)| u16::try_from(slot).ok())
}

/// A function's instance initializer (`Function::instance_initializer`), as the program's
/// records tell it.
#[derive(Clone, Copy, Debug)]
pub(super) enum InstanceInitializer {
    Known(Option<u32>),
    /// A released class constructor's record, which names it, is not at hand.
    Unknown,
}

/// What each function's closures read of the environments they close over
/// (`bytecode::ClosureReads`), from the facts every function's compilation recorded.
/// `dynamic_scopes` is `dynamic_name_scopes` of `facts`; `initializers` gives each
/// function's instance initializer.
pub(super) fn closure_reads(
    facts: &[FunctionFacts],
    dynamic_scopes: &[bool],
    initializers: &[InstanceInitializer],
) -> crate::bytecode::ClosureReads {
    let functions: Vec<crate::bytecode::FunctionReadFacts<'_>> = facts
        .iter()
        .enumerate()
        .map(|(id, function)| {
            let initializer = initializers.get(id).copied().unwrap_or(InstanceInitializer::Unknown);
            crate::bytecode::FunctionReadFacts {
                parent: function.parent,
                is_arrow: function.is_arrow,
                captures: &function.captures,
                named_slots: &function.named_slots,
                dynamic: dynamic_scopes.get(id).copied().unwrap_or(true)
                    || matches!(initializer, InstanceInitializer::Unknown),
                instance_initializer: match initializer {
                    InstanceInitializer::Known(initializer) => initializer,
                    InstanceInitializer::Unknown => None,
                },
            }
        })
        .collect();
    crate::bytecode::ClosureReads::derive(&functions)
}

/// The slots of function `owner` that the functions in `descendants` capture, before the
/// owner's own additions (`apply_function`).
pub(super) fn captured_by(owner: usize, facts: &[FunctionFacts], descendants: std::ops::Range<usize>) -> Vec<u16> {
    let mut captured = Vec::new();
    for id in descendants {
        for &(distance, slot) in facts[id].captures.iter() {
            if ancestor(facts, id, distance as usize) == Some(owner)
                && slot < facts[owner].locals
            {
                captured.push(slot);
            }
        }
    }
    captured
}

/// The layout inputs of every function, from their facts. `dynamic` is
/// `dynamic_name_resolution` of `facts`; `arguments` gives a function's mapped `arguments`
/// slot and parameter count when its record still exists.
pub(super) fn inputs(
    facts: &[FunctionFacts],
    dynamic: &[bool],
    arguments: impl Fn(usize) -> Option<(u16, u16)>,
) -> Vec<LayoutInputs> {
    let dynamic_ancestors = dynamic_scope_ancestors(facts, dynamic);
    let immutable = immutable_declarations(facts, &dynamic_ancestors);
    let mut captured = facts.iter().map(|_| Vec::new()).collect::<Vec<_>>();
    for (id, function) in facts.iter().enumerate() {
        for &(distance, slot) in function.captures.iter() {
            if let Some(owner) = ancestor(facts, id, distance as usize)
                && slot < facts[owner].locals
            {
                captured[owner].push(slot);
            }
        }
    }
    // A capture of the parent's slot (distance 1) that the parent declares and nothing writes is the declaration's identity.
    let declared_captures = facts.iter().map(|function| -> Vec<u16> { match function.parent {
        Some(parent) => function.captures.iter()
            .filter(|(distance, slot)| *distance == 1 && immutable[parent as usize].binary_search(slot).is_ok())
            .map(|(_, slot)| *slot).collect(),
        None => Vec::new(),
    } });
    captured
        .into_iter()
        .enumerate()
        .zip(dynamic_ancestors)
        .zip(declared_captures)
        .map(|(((id, captured), dynamic_descendant), declared_captures)| LayoutInputs {
            captured: complete_captured(captured, arguments(id)),
            dynamic_descendant,
            split_root: false,
            declared_captures,
        })
        .collect()
}

/// The slots of each function's declarations that nothing writes, sorted: a declaration the function stores once
/// (its hoisting), with no other store to its slot in the function, none from a nested function, and no dynamic
/// name resolution around it (`dynamic_ancestors`: an `eval` or `with` may write any binding it encloses).
fn immutable_declarations(facts: &[FunctionFacts], dynamic_ancestors: &[bool]) -> Vec<Vec<u16>> {
    let mut written_below: Vec<Vec<u16>> = facts.iter().map(|_| Vec::new()).collect();
    for (id, function) in facts.iter().enumerate() {
        for &(distance, slot) in function.stores.iter().filter(|(distance, _)| *distance > 0) {
            if let Some(owner) = ancestor(facts, id, distance as usize) {
                written_below[owner].push(slot);
            }
        }
    }
    facts.iter().enumerate().map(|(id, function)| {
        let stored = |slot: u16| function.stores.iter().filter(|store| **store == (0, slot)).count();
        let mut immutable: Vec<u16> = function.declared.iter().copied()
            .filter(|&slot| !dynamic_ancestors[id] && stored(slot) == 1 && !written_below[id].contains(&slot))
            .collect();
        immutable.sort_unstable();
        immutable.dedup();
        immutable
    }).collect()
}

/// A function's captured slots, completed: a mapped `arguments` object aliases the
/// parameters, so capturing one captures it too.
pub(super) fn complete_captured(mut captured: Vec<u16>, arguments: Option<(u16, u16)>) -> Vec<u16> {
    if let Some((arguments_slot, params)) = arguments
        && captured.iter().any(|slot| *slot < params)
    {
        captured.push(arguments_slot);
    }
    captured.sort_unstable();
    captured.dedup();
    captured
}

/// The mapped `arguments` slot and parameter count of `function`, which `complete_captured`
/// reads.
pub(super) fn mapped_arguments(function: &Function) -> Option<(u16, u16)> {
    function
        .arguments_alias_parameters()
        .then_some(function.arguments_slot)
        .flatten()
        .map(|slot| (slot, function.params))
}

/// Lays out the activation of function `id` from its own code and its `inputs`.
pub(super) fn apply_function(
    function: &mut Function,
    id: usize,
    inputs: &LayoutInputs,
    atoms: &CompilerAtoms<'_>,
) {
    function.declared_captures = crate::heap::ThinBoxSlice::from_slice(&inputs.declared_captures);
    let mut captured = inputs.captured.clone();
    // A module body's `captured` includes the bindings its module environment holds
    // (`Function::module_environment_slots`), closures or not, so it always splits.
    if !inputs.split_root && !has_closure(function) {
        // No closure of this activation can exist, and no nested function captures
        // its locals: a per-iteration or block copy of the environment is unobservable
        // under the same layout conditions that let a closure's activation split.
        if captured.is_empty() && clones_are_unobservable(function, id, inputs, atoms) {
            remove_unobservable_clones(function, &captured);
        }
        return;
    }

    let can_split = selective_layout_is_safe(function, id, inputs, atoms);
    if can_split {
        // The runtime finds compiler bindings such as class home objects by name in
        // the environment, so they live there like captured bindings; the other
        // locals stay in registers.
        captured.extend(named_environment_slots(function, atoms));
        captured.sort_unstable();
        captured.dedup();
        // An `arguments` binding that no instruction addresses and no closure captures is
        // unobservable, as `Compiler::compile_function` finds for a closure-free body.
        if let Some(slot) = function.arguments_slot
            && captured.binary_search(&slot).is_err()
            && !function
                .instructions()
                .any(|instruction| instruction.addressed_local_slots().any(|local| local == slot))
        {
            (function.arguments_slot, function.rest_alias) = (None, false);
        }
        function.selective_capture_slots = Some(crate::heap::ThinBoxSlice::from_slice(&captured));
        rewrite_local_ownership(function, &captured);
        remove_unobservable_clones(function, &captured);
    } else {
        function.selective_capture_slots = None;
        rewrite_all_locals_to_environment(function);
    }
}

/// Functions whose name bindings a dynamic name lookup may consult: those enclosing dynamic
/// name resolution (`dynamic_scope_ancestors`), whose environments such a lookup walks, and
/// those it encloses, whose by-name loads no longer resolve statically.
/// `dynamic` is `dynamic_name_resolution` of `functions`.
pub(super) fn dynamic_name_scopes(functions: &[FunctionFacts], dynamic: &[bool]) -> Vec<bool> {
    let mut reachable = dynamic_scope_ancestors(functions, dynamic);
    for (id, consults_dynamic) in reachable.iter_mut().enumerate() {
        let mut current = Some(id);
        while let Some(function) = current {
            if dynamic[function] {
                *consults_dynamic = true;
                break;
            }
            current = functions[function].parent.map(|parent| parent as usize);
        }
    }
    reachable
}

/// Functions resolving names dynamically themselves: direct eval, by-name resolution, a
/// `with` statement, or an enclosing function's binding sites.
pub(super) fn dynamic_name_resolution(functions: &[FunctionFacts]) -> Vec<bool> {
    let inherited_dynamic_scope = inherited_dynamic_scope(functions);
    functions
        .iter()
        .enumerate()
        .map(|(id, function)| {
            function.dynamic_access || inherited_dynamic_scope[id] || function.contains_with
        })
        .collect()
}

/// Functions enclosing dynamic name resolution, including a `with` body's by-name
/// loads, which may denote any ancestor's binding and so need every ancestor's
/// locals in its environment.
fn dynamic_scope_ancestors(functions: &[FunctionFacts], dynamic: &[bool]) -> Vec<bool> {
    let mut unsafe_layout = vec![false; functions.len()];
    for (id, &is_dynamic) in dynamic.iter().enumerate().take(functions.len()) {
        if !is_dynamic {
            continue;
        }
        let mut current = Some(id);
        while let Some(owner) = current {
            unsafe_layout[owner] = true;
            current = functions[owner].parent.map(|parent| parent as usize);
        }
    }
    unsafe_layout
}

fn inherited_dynamic_scope(functions: &[FunctionFacts]) -> Vec<bool> {
    let mut inherited = vec![false; functions.len()];
    for (id, function) in functions.iter().enumerate() {
        let Some(parent) = function.parent.map(|parent| parent as usize) else {
            continue;
        };
        inherited[id] = inherited[parent] || functions[parent].binding_sites;
    }
    inherited
}

/// Whether the function's activation may keep its uncaptured locals out of its environment:
/// a nested function, or a module body, whose environment nothing outside it reads by name.
fn selective_layout_is_safe(function: &Function, id: usize, inputs: &LayoutInputs, atoms: &CompilerAtoms<'_>) -> bool {
    !function.is_async && !function.is_generator && clones_are_unobservable(function, id, inputs, atoms)
}

/// `selective_layout_is_safe` apart from suspension: what a closure-free activation needs for
/// its per-iteration and block environment copies to be unobservable. A suspension saves the
/// frame's locals with it and names no environment slot, so an async or generator body's
/// copies are as unobservable as any other's.
fn clones_are_unobservable(function: &Function, id: usize, inputs: &LayoutInputs, atoms: &CompilerAtoms<'_>) -> bool {
    activation_is_private(function, id, inputs)
        // Each argument arrives in a slot of its own, which no closure names: a simple list's
        // or a directly binding rest list's parameter, or the hidden slot of a list binding
        // its names apart (`Function::arguments_own_their_slots`), whose bindings are locals.
        && function.arguments_own_their_slots(|atom| atoms.get(atom))
}

/// Whether nothing outside the function's own code reads its environment by name: a nested
/// function or a module body, with no dynamic scope access and no construction protocol that
/// names its slots.
fn activation_is_private(function: &Function, id: usize, inputs: &LayoutInputs) -> bool {
    let nested = id != 0 && function.parent.is_some();
    let module_body = inputs.split_root && function.parent.is_none();
    (nested || module_body)
        && !inputs.dynamic_descendant
        && function.construction_is_private()
        && !function.has_dynamic_scope_access()
}

fn has_closure(function: &Function) -> bool {
    function
        .code
        .iter()
        .any(|instruction| instruction.op().traits().has(Traits::MAKES_CLOSURE))
        || function
            .wide
            .iter()
            .any(|instruction| instruction.op().traits().has(Traits::MAKES_CLOSURE))
}

/// The environment twin of a relocatable frame-local access, `LoadLocal` or `StoreLocal`.
fn env_local_twin(access: Op) -> Op {
    if access.traits().has(Traits::FRAME_LOCAL_LOAD) {
        ENV_LOCAL_LOAD_OP
    } else {
        ENV_LOCAL_STORE_OP
    }
}

fn rewrite_local_ownership(function: &mut Function, captured: &[u16]) {
    for instruction in &mut function.code {
        rewrite_if_captured(instruction, captured);
    }
    for instruction in &mut function.wide {
        if instruction.op().is_relocatable_local_access()
            && u16::try_from(instruction.local_slot())
                .is_ok_and(|slot| captured.binary_search(&slot).is_ok())
        {
            instruction.set_op(env_local_twin(instruction.op()));
        }
    }
}

/// A per-iteration environment copy is observable only through a closure holding one of its
/// fresh bindings. At a site that runs again in the same activation (`may_repeat`), a closure
/// made in one run must keep the bindings the next run replaces. At a site that runs at most
/// once, a closure made before it (a `for (let ...)` initializer's) keeps the bindings the copy
/// leaves behind, which later writes to the copy must not reach. When no fresh binding of a
/// clone site is captured, or the site runs once with no closure made before it, every closure
/// sees the same binding objects with or without the copy, so the site does nothing. A copy
/// that stays would also be the environment every later closure of the activation captures.
fn remove_unobservable_clones(function: &mut Function, captured: &[u16]) {
    let observable = |plan: &crate::bytecode::EnvironmentClone| {
        plan.fresh
            .iter()
            .any(|slot| captured.binary_search(slot).is_ok())
    };
    let backward = function.backward_transfers();
    // A site that runs once runs after every instruction before it in code order: only
    // forward transfers reach it.
    let first_closure = function
        .code
        .iter()
        .position(|instruction| instruction.op().traits().has(Traits::MAKES_CLOSURE))
        .unwrap_or(usize::MAX)
        .min(if function.wide.iter().any(|instruction| instruction.op().traits().has(Traits::MAKES_CLOSURE)) { 0 } else { usize::MAX });
    for (pc, instruction) in function.code.iter_mut().enumerate() {
        if instruction.op().traits().has(Traits::ENVIRONMENT_CLONE)
            && (!observable(&function.environment_clones[instruction.environment_clone_index()])
                || (!may_repeat(&backward, pc) && first_closure > pc))
        {
            *instruction = crate::bytecode::Instr::new(NOP_OP, 0, 0, 0, 0);
        }
    }
}

fn rewrite_if_captured(instruction: &mut crate::bytecode::Instr, captured: &[u16]) {
    if !instruction.op().is_relocatable_local_access()
        || !u16::try_from(instruction.local_slot())
            .is_ok_and(|slot| captured.binary_search(&slot).is_ok())
    {
        return;
    }
    instruction.set_op(env_local_twin(instruction.op()));
}

fn rewrite_all_locals_to_environment(function: &mut Function) {
    for instruction in &mut function.code {
        if instruction.op().is_relocatable_local_access() {
            instruction.set_op(env_local_twin(instruction.op()));
        }
    }
    for instruction in &mut function.wide {
        if instruction.op().is_relocatable_local_access() {
            instruction.set_op(env_local_twin(instruction.op()));
        }
    }
}

fn ancestor(functions: &[FunctionFacts], mut function: usize, distance: usize) -> Option<usize> {
    for _ in 0..distance {
        function = functions.get(function)?.parent? as usize;
    }
    Some(function)
}
