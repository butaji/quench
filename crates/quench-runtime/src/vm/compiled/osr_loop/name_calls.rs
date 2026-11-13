//! A by-name call in a region: `LoadNameCall` loads a global binding's function into its
//! result register and `undefined` into the call's receiver register, which the `Call` after
//! it reads as `this`. The callee is the entry capture the binding's value materializes
//! (`GLOBAL_DEPTH`); a binding a `with` object could answer is never admitted, because
//! `rewrite_entry_reads` keeps a dynamic-scope function's name loads.

use super::*;

/// The second register a by-name call defines: the receiver its `Call` reads.
pub(super) fn second_definition(instruction: WideInstruction) -> Option<Register> {
    (instruction.op().class() == Class::LoadNameCall).then(|| instruction.register_b())
}

/// The register and definition id of each definition `instruction` writes.
pub(super) fn definitions(
    entry_count: usize,
    offset: usize,
    instruction: WideInstruction,
) -> impl Iterator<Item = (Register, Definition)> {
    let result = slots::result(entry_count, offset);
    let second = slots::second(entry_count, offset);
    number_definition(instruction).map(|register| (register, result))
        .into_iter()
        .chain(second_definition(instruction).map(|register| (register, second)))
}

/// The definition id `instruction` writes to `register`: its result, or its second register.
pub(super) fn definition_of(
    entry_count: usize,
    offset: usize,
    instruction: WideInstruction,
    register: Register,
) -> Option<Definition> {
    definitions(entry_count, offset, instruction)
        .find(|(written, _)| *written == register)
        .map(|(_, definition)| definition)
}

/// The web of the instruction at `pc`'s second definition.
pub(super) fn defined_second(webs: &Webs, region: &LoopRegion, pc: u32) -> Option<Definition> {
    second_definition(region.instructions[(pc - region.header) as usize])
        .map(|_| webs.find(slots::second(region.entry_count(), (pc - region.header) as usize)))
}

/// The registers a call writes as values: a by-name call's callee (a function) and receiver
/// (`undefined`), and a native call's result (the callee's value) and callee.
pub(super) fn tagged_registers(instructions: &[WideInstruction]) -> impl Iterator<Item = Register> + '_ {
    let by_name = instructions.iter()
        .filter(|instruction| instruction.op().class() == Class::LoadNameCall)
        .flat_map(|instruction| [instruction.result_register(), instruction.register_b()]);
    by_name
}

/// Each web's starting representation, with each second definition's web tagged: a by-name
/// call's receiver is `undefined`.
pub(super) fn web_representations(region: &LoopRegion, webs: &Webs) -> Vec<Representation> {
    let mut representations = vec![Representation::Int32; webs.parent.len()];
    for (offset, instruction) in region.instructions.iter().enumerate() {
        let definition = if second_definition(*instruction).is_some() {
            Some(slots::second(region.entry_count(), offset))
        } else {
            None
        };
        if let Some(definition) = definition {
            representations[webs.find(definition)] = Representation::Tagged;
        }
    }
    representations
}

/// Types each by-name call's second definition: the receiver is `undefined`. The callee is
/// the instruction's result, which the shared definition loop counts and types.
pub(super) fn note_definitions(
    region: &LoopRegion,
    webs: &Webs,
    counts: &mut [usize],
    types: &mut [Type],
    values: &mut [ValueFact],
) {
    for (offset, instruction) in region.instructions.iter().copied().enumerate() {
        if !webs.reachable[offset] {
            continue;
        }
        let pc = region.header + offset as u32;
        if number_definition(instruction).is_some() {
            counts[webs.defined(region, pc)] += 1;
        }
        if second_definition(instruction).is_none() {
            continue;
        }
        let definition = webs.find(slots::second(region.entry_count(), offset));
        counts[definition] += 1;
        types[definition] = types[definition].join(Type::immediate(Value::UNDEFINED));
        values[definition].ty = values[definition].ty.join(Type::immediate(Value::UNDEFINED));
    }
}

/// The global binding `instruction` (a `LoadNameCall`) reads, when `globals` lists its site:
/// such a callee is `CalleeRead::Global`.
pub(super) fn global_call(
    instruction: WideInstruction,
    globals: &[(crate::bytecode::Atom, u16)],
) -> Option<(crate::bytecode::Atom, u16)> {
    (instruction.op().class() == Class::LoadNameCall)
        .then(|| (instruction.atom_index(), instruction.cache_site_index()))
        .filter(|site| globals.contains(site))
}

/// How a kept callee read resolves: its global binding, a depth-0 capture, or a field chain.
pub(super) fn callee_read(global: Option<(crate::bytecode::Atom, u16)>, capture: Option<u16>) -> CalleeRead {
    match (global, capture) {
        (Some((atom, site)), _) => CalleeRead::Global { atom, site },
        (None, Some(slot)) => CalleeRead::Capture(slot),
        (None, None) => CalleeRead::Chain,
    }
}

/// The guard of a call whose callee read is kept: a by-name call checks its global binding,
/// any other compares the read's value, both at region offset `at`.
pub(super) fn read_guard(
    read: WideInstruction,
    globals: &[(crate::bytecode::Atom, u16)],
    at: u32,
) -> InlineGuard {
    match global_call(read, globals) {
        Some((atom, site)) => InlineGuard::Global { atom, site, read: at },
        None => InlineGuard::AtRead(at),
    }
}

/// Whether an inlined callee's `body` may run in place: plain instructions, and — for a callee
/// read by capture or by name, whose call passes no meaningful receiver — no `this` at all.
pub(super) fn admits_body(body: &[WideInstruction], sites: &[FieldSite], receiver_independent: bool) -> Option<()> {
    body.iter().all(|instruction| inlinable(instruction.op())
        && (instruction.result_flags() == 0 || inlined_returning(*instruction) || inlined_set_this(*instruction, sites).is_some()))
        .then_some(())?;
    if receiver_independent {
        body.iter().all(|instruction| !names_this(*instruction, sites)).then_some(())?;
    }
    Some(())
}

/// Whether `instruction` reads or writes the activation's `this`.
fn names_this(instruction: WideInstruction, sites: &[FieldSite]) -> bool {
    instruction.op().is_load_this()
        || instruction.op().is_set_this_field()
        || instruction.writes_current_this()
        || (instruction.op().is_get_field() && field_base(instruction, sites) == Some(FieldBase::THIS))
}

/// The entry capture holding the function `instruction`'s global binding names.
pub(super) fn global_capture_index(region: &LoopRegion, instruction: WideInstruction) -> Option<usize> {
    let site = (instruction.atom_index(), instruction.cache_site_index());
    region.capture_globals.iter().position(|global| *global == Some(site))
}

impl Admission<'_> {
    /// Admits a by-name call: the callee is the global binding's entry capture, tagged, and
    /// the receiver the `Call` reads is `undefined`.
    pub(super) fn admit_name_call(&mut self, instruction: WideInstruction) -> Option<()> {
        instruction.result_flags().eq(&0).then_some(())?;
        self.tagged(instruction.result_register())?;
        self.tagged(instruction.register_b())?;
        self.global_capture((instruction.atom_index(), instruction.cache_site_index()))?;
        Some(())
    }
}

impl Lowering<'_, '_> {
    /// The inlined call whose kept callee read is the instruction at `pc`.
    fn read_frame(&self, pc: u32) -> Option<InlineFrame> {
        let offset = pc - self.region.header;
        self.region.inlining.as_ref()?.frames.iter().copied().find(|frame| matches!(frame.guard,
            InlineGuard::AtRead(at) | InlineGuard::Global { read: at, .. } if at == offset))
    }

    /// Lowers a by-name call to its callee's entry capture and an `undefined` receiver, or a
    /// capture read to its capture.
    pub(super) fn capture_read(&mut self, pc: u32, instruction: WideInstruction) -> Option<()> {
        if instruction.op().class() == Class::Call {
            return self.native_call(pc, instruction);
        }
        if instruction.op().class() == Class::LoadNameCall {
            let index = global_capture_index(self.region, instruction)?;
            let callee = self.capture(index);
            // Inlined by name: the body is the callee's, so a rebound binding leaves for the
            // lane at this read, which re-runs the call as written.
            if let Some(frame) = self.read_frame(pc) {
                let other = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, callee.value, frame.callee.to_bits() as i64);
                self.guard(pc, other);
            }
            self.define(instruction.result_register(), callee);
            let receiver = self.read(Source::Tagged(Value::UNDEFINED));
            self.define_second(instruction.register_b(), receiver);
            return self.fall_through(pc);
        }
        let index = capture_index(self.region, instruction)?;
        let value = self.capture(index);
        self.define(instruction.result_register(), value);
        self.fall_through(pc)
    }
}
