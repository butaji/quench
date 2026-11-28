//! Field-path analysis of a loop region: the method call records, the field paths a body reads, and the registers those paths hold.
use super::*;

/// What a plain region does at a call whose callee a field read gives (a method call): leave
/// the loop to the inliner, which makes its region once the site is hot (a call boundary,
/// `RegionAnalysisFailure::is_call_boundary`), or leave at the call as at any other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MethodCalls {
    Inliner,
    Exit,
}

impl MethodCalls {
    /// Whether a region with call receivers `objects` (an inlining one, which leaves at a
    /// call it kept) leaves its method calls to the inliner.
    pub(super) fn inlined(self, objects: &std::collections::BTreeSet<Register>) -> bool {
        self == Self::Inliner && objects.is_empty()
    }
}

/// Whether `instruction` reads a field named `atom` at any step: its own name, or one of its nested
/// site's (`this.state.pos` reads `state`, then `pos`). A region store of the name must see such a
/// read too, or the read keeps its entry snapshot past the store (Babylon's `skipLineComment` read
/// `this.state.pos` stale after `++this.state.pos` in u-jit and ran past the line's end).
pub(super) fn reads_name(instruction: WideInstruction, sites: &[FieldSite], atom: quench_runtime::bytecode::Atom) -> bool {
    quench_runtime::staged::reads_named_field(instruction.op()) && match instruction.field_lookup() {
        Some(FieldLookup::Atom { atom: read, .. }) => read == atom,
        Some(FieldLookup::Site(index)) => sites.get(index).is_none_or(|site| {
            site.first.0 == atom || site.second.is_some_and(|(read, _)| read == atom) || site.sink.is_some_and(|(read, _)| read == atom)
        }),
        None => false,
    }
}

/// The base a `GetField` reads its first field from, its own or its nested site's.
pub(super) fn field_base(instruction: WideInstruction, sites: &[FieldSite]) -> Option<FieldBase> {
    match instruction.field_lookup()? {
        FieldLookup::Atom { base, .. } => Some(base),
        FieldLookup::Site(index) => Some(sites.get(index)?.base),
    }
}

/// The path a `GetField` reads, a register base that `held` names (`held_registers`) read
/// through the path that gave it its value; a nested site (`FieldLookup::Site`) is its steps.
pub(super) fn field_path(
    instruction: WideInstruction,
    sites: &[FieldSite],
    held: &std::collections::BTreeMap<Register, FieldPath>,
) -> Option<FieldPath> {
    (quench_runtime::staged::reads_named_field(instruction.op()) && instruction.result_flags() == 0).then_some(())?;
    let from = |base: FieldBase| -> Option<FieldPath> {
        if base == FieldBase::THIS {
            return Some(FieldPath { root: FieldReceiver::This, steps: Vec::new() });
        }
        let register = base.register_index()?;
        Some(held.get(&register).cloned().unwrap_or(FieldPath { root: FieldReceiver::Register(register), steps: Vec::new() }))
    };
    match instruction.field_lookup()? {
        FieldLookup::Atom { atom, base, cache_site } => {
            let mut path = from(base)?;
            path.steps.push((atom, cache_site));
            Some(path)
        }
        FieldLookup::Site(index) => {
            let site = sites.get(index)?;
            site.sink.is_none().then_some(())?;
            let mut path = from(site.base)?;
            path.steps.push(site.first);
            path.steps.extend(site.second);
            Some(path)
        }
    }
}

/// The index in `fields` of the field whose value `path` reads, adding each step the
/// fields lack after the one it reads from: every step but the last is a receiver, so
/// tagged, and the last is `tagged` as its read's result.
pub(super) fn intern_field(fields: &mut Vec<FieldUse>, path: &FieldPath, tagged: bool) -> usize {
    let mut receiver = path.root;
    let mut index = usize::MAX;
    for (step, (atom, cache_site)) in path.steps.iter().copied().enumerate() {
        let tagged = tagged || step + 1 < path.steps.len();
        index = match fields
            .iter()
            .position(|field| field.receiver == receiver && field.atom == atom && field.tagged == tagged)
        {
            Some(index) => index,
            None => {
                fields.push(FieldUse { receiver, atom, cache_site, tagged });
                fields.len() - 1
            }
        };
        receiver = FieldReceiver::Field(index);
    }
    index
}

/// The registers a region reads as objects (`object_reads`, and `objects`) that hold a
/// field's value at every read: each definition of one in the region is a `GetField` of
/// one path, and none is live at the header (`header_live`), so every read follows a
/// definition. As the region writes no field, the value is the one the path reads at
/// entry. With no liveness, none is. Each register holding a capture's value
/// (`receivers::captured`) comes too, rooted at the capture with no steps.
pub(super) fn held_registers(
    instructions: &[WideInstruction],
    sites: &[FieldSite],
    header_live: Option<RegisterSet>,
    objects: &std::collections::BTreeSet<Register>,
) -> std::collections::BTreeMap<Register, FieldPath> {
    let Some(header_live) = header_live else { return std::collections::BTreeMap::new() };
    // A register holding a capture's value roots the paths read through it, with no steps.
    let mut held = receivers::captured(instructions, header_live);
    let definitions = |register: Register| {
        instructions.iter().copied().filter(move |instruction| number_definition(*instruction) == Some(register))
    };
    let pending: std::collections::BTreeSet<Register> = instructions
        .iter()
        .copied()
        .filter_map(object_reads)
        .chain(objects.iter().copied())
        .filter(|register| {
            !quench_runtime::compile::liveness::register_in(header_live, usize::from(*register))
                && definitions(*register).next().is_some()
                && definitions(*register).all(|instruction| quench_runtime::staged::reads_named_field(instruction.op()))
        })
        .collect();
    // A path through a register still pending waits for it; a register whose definitions
    // read different paths, or wait on one another, holds no field. Each pass decides the
    // registers no pending one blocks, and records their paths in `held`.
    let mut rounds = Passes {
        pass: |pending: &std::collections::BTreeSet<Register>| {
            let mut decided = Vec::new();
            for register in pending.iter().copied() {
                let mut paths = Vec::new();
                for instruction in definitions(register) {
                    let waits = field_base(instruction, sites).and_then(FieldBase::register_index);
                    if waits.is_some_and(|base| pending.contains(&base)) {
                        paths.clear();
                        break;
                    }
                    paths.push(field_path(instruction, sites, &held));
                }
                if paths.is_empty() {
                    continue;
                }
                let first = paths[0].clone();
                let one = first.as_ref().filter(|first| paths.iter().all(|path| path.as_ref().is_some_and(|path| path.reads_as(first))));
                decided.push((register, one.cloned()));
            }
            let mut next = pending.clone();
            for (register, path) in decided {
                next.remove(&register);
                if let Some(path) = path {
                    held.insert(register, path);
                }
            }
            next
        },
        edge: [0],
        initial: pending,
    };
    quench_runtime::dataflow::solve(&mut rounds);
    drop(rounds);
    held
}
