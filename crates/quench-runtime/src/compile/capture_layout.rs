use super::CompilerAtoms;

use crate::bytecode::{Function, Op, WITH_ENTER_BINDING};

/// What the layout of a function's activation depends on beyond its own code: the slots of
/// its locals that functions nested in it capture, and whether one of them resolves names
/// at run time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayoutInputs {
    pub(crate) captured: Vec<u16>,
    pub(crate) dynamic_descendant: bool,
    /// Whether the function is a module's body. Its module-scope bindings live in the module
    /// environment, which importers and the linker address by slot, and are in `captured`;
    /// its block bindings are as private to its activation as a function's.
    pub(crate) module_root: bool,
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
            .filter(|instruction| matches!(instruction.op(), Op::LoadCapture | Op::StoreCapture))
            .map(|instruction| (instruction.capture_depth(), instruction.capture_slot()))
            .chain(
                function
                    .wide
                    .iter()
                    .filter(|instruction| matches!(instruction.op(), Op::LoadCapture | Op::StoreCapture))
                    .map(|instruction| (instruction.capture_depth(), instruction.capture_slot())),
            )
            .map(|(depth, slot)| (u32::from(depth) + 1, slot))
            .collect();
        captures.sort_unstable();
        captures.dedup();
        Self {
            parent: function.parent,
            is_arrow: function.is_arrow,
            locals: function.locals,
            dynamic_access: function.has_dynamic_scope_access(),
            contains_with: function.contains_with_statement(is_with_enter),
            binding_sites: !function.binding_sites.is_empty(),
            captures: captures.into(),
            named_slots: slots_read_by_name(function, atoms).collect(),
        }
    }
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
                crate::bytecode::local_name_read_by_name(name, function.environment_atoms.contains(*atom))
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
    captured
        .into_iter()
        .enumerate()
        .zip(dynamic_ancestors)
        .map(|((id, captured), dynamic_descendant)| LayoutInputs {
            captured: complete_captured(captured, arguments(id)),
            dynamic_descendant,
            module_root: false,
        })
        .collect()
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
    let mut captured = inputs.captured.clone();
    // A module body's `captured` includes the bindings its module environment holds
    // (`Function::module_environment_slots`), closures or not, so it always splits.
    if !inputs.module_root && !has_closure(function) {
        // No closure of this activation can exist, and no nested function captures
        // its locals: a per-iteration or block copy of the environment is unobservable
        // under the same layout conditions that let a closure's activation split.
        if captured.is_empty() && selective_layout_is_safe(function, id, inputs, atoms)
        {
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
            function.arguments_slot = None;
        }
        function.selective_capture_slots =
            Some(crate::heap::ThinBoxSlice::from_exact_iter(captured.iter().copied()));
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
    for id in 0..functions.len() {
        let mut current = Some(id);
        while let Some(function) = current {
            if dynamic[function] {
                reachable[id] = true;
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
    for id in 0..functions.len() {
        if !dynamic[id] {
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
    let nested = id != 0 && function.parent.is_some();
    let module_body = inputs.module_root && function.parent.is_none();
    (nested || module_body)
        && !inputs.dynamic_descendant
        && !function.is_async
        && !function.is_generator
        && !function.is_class_constructor
        && !function.derived_constructor
        && !function.class_initializer
        // A rest list binding directly names each parameter's own slot, as a simple one does.
        && function.parameters_own_their_slots(|atom| atoms.get(atom))
        && !function.has_dynamic_scope_access()
}

fn has_closure(function: &Function) -> bool {
    function
        .code
        .iter()
        .any(|instruction| instruction.op() == Op::MakeClosure)
        || function
            .wide
            .iter()
            .any(|instruction| instruction.op() == Op::MakeClosure)
}

fn rewrite_local_ownership(function: &mut Function, captured: &[u16]) {
    for instruction in &mut function.code {
        rewrite_if_captured(instruction, captured);
    }
    for instruction in &mut function.wide {
        if matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal)
            && u16::try_from(instruction.local_slot())
                .is_ok_and(|slot| captured.binary_search(&slot).is_ok())
        {
            instruction.set_op(if instruction.op() == Op::LoadLocal {
                Op::LoadEnvLocal
            } else {
                Op::StoreEnvLocal
            });
        }
    }
}

/// A per-iteration environment copy is observable only through a closure holding one of its
/// fresh bindings, and only when its site runs again in the same activation: a closure made
/// after one run must then keep the bindings the next run replaces. When no fresh binding of
/// a clone site is captured, or the site runs at most once (`may_repeat`), every closure sees
/// the same binding objects with or without the copy, so the site does nothing. A copy that
/// stays would also be the environment every later closure of the activation captures.
fn remove_unobservable_clones(function: &mut Function, captured: &[u16]) {
    let observable = |plan: &crate::bytecode::EnvironmentClone| {
        plan.fresh
            .iter()
            .any(|slot| captured.binary_search(slot).is_ok())
    };
    let backward = backward_transfers(function);
    for (pc, instruction) in function.code.iter_mut().enumerate() {
        if instruction.op() == Op::CloneEnv
            && (!observable(&function.environment_clones[instruction.environment_clone_index()])
                || !may_repeat(&backward, pc))
        {
            *instruction = crate::bytecode::Instr::new(Op::Nop, 0, 0, 0, 0);
        }
    }
}

/// Every control transfer of `function` to an instruction at or before its source, as
/// (target, source): jumps, branch table entries (which are jumps), and exception and
/// `finally` continuations, taken from the last instruction their range covers.
fn backward_transfers(function: &Function) -> Vec<(usize, usize)> {
    use crate::bytecode::ControlFlowLayout;
    let mut transfers = Vec::new();
    for (pc, instruction) in function.instructions().enumerate() {
        if matches!(
            instruction.op().control_flow_layout(),
            ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump
        ) && instruction.jump_target() as usize <= pc
        {
            transfers.push((instruction.jump_target() as usize, pc));
        }
    }
    for handler in function.handlers.iter() {
        let last = handler.end.saturating_sub(1) as usize;
        for target in std::iter::once(handler.target).chain(handler.return_target) {
            if target as usize <= last {
                transfers.push((target as usize, last));
            }
        }
    }
    transfers
}

/// Whether the instruction at `pc` may run more than once in one activation. A cycle through
/// `pc` must leave it for an instruction before it, by a transfer from at or after `pc` to at
/// or before it, which `backward` lists.
fn may_repeat(backward: &[(usize, usize)], pc: usize) -> bool {
    backward.iter().any(|&(target, source)| target <= pc && pc <= source)
}

fn rewrite_if_captured(instruction: &mut crate::bytecode::Instr, captured: &[u16]) {
    if !matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal)
        || !u16::try_from(instruction.local_slot())
            .is_ok_and(|slot| captured.binary_search(&slot).is_ok())
    {
        return;
    }
    instruction.set_op(if instruction.op() == Op::LoadLocal {
        Op::LoadEnvLocal
    } else {
        Op::StoreEnvLocal
    });
}

fn rewrite_all_locals_to_environment(function: &mut Function) {
    for instruction in &mut function.code {
        if matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal) {
            instruction.set_op(if instruction.op() == Op::LoadLocal {
                Op::LoadEnvLocal
            } else {
                Op::StoreEnvLocal
            });
        }
    }
    for instruction in &mut function.wide {
        if matches!(instruction.op(), Op::LoadLocal | Op::StoreLocal) {
            instruction.set_op(if instruction.op() == Op::LoadLocal {
                Op::LoadEnvLocal
            } else {
                Op::StoreEnvLocal
            });
        }
    }
}

fn ancestor(functions: &[FunctionFacts], mut function: usize, distance: usize) -> Option<usize> {
    for _ in 0..distance {
        function = functions.get(function)?.parent? as usize;
    }
    Some(function)
}
