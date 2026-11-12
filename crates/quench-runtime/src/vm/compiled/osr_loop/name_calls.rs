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

/// The registers a by-name call writes: its callee, a function, and its receiver,
/// `undefined`.
pub(super) fn tagged_registers(instructions: &[WideInstruction]) -> impl Iterator<Item = Register> + '_ {
    instructions.iter()
        .filter(|instruction| instruction.op().class() == Class::LoadNameCall)
        .flat_map(|instruction| [instruction.result_register(), instruction.register_b()])
}

/// Each web's starting representation, with each second definition's web tagged: a by-name
/// call's receiver is `undefined`.
pub(super) fn web_representations(region: &LoopRegion, webs: &Webs) -> Vec<Representation> {
    let mut representations = vec![Representation::Int32; webs.parent.len()];
    for (offset, instruction) in region.instructions.iter().enumerate() {
        if second_definition(*instruction).is_some() {
            representations[webs.find(slots::second(region.entry_count(), offset))] = Representation::Tagged;
        }
    }
    representations
}

/// Counts and types each definition, the by-name call's two included: its callee is any
/// value, its receiver `undefined`.
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
        let Some(_) = second_definition(instruction) else { continue };
        for definition in [webs.defined(region, pc), webs.find(slots::second(region.entry_count(), offset))] {
            counts[definition] += 1;
            types[definition] = types[definition].join(Type::ANY);
            values[definition].ty = values[definition].ty.join(Type::ANY);
        }
    }
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
    /// Lowers a by-name call to its callee's entry capture and an `undefined` receiver, or a
    /// capture read to its capture.
    pub(super) fn capture_read(&mut self, pc: u32, instruction: WideInstruction) -> Option<()> {
        if instruction.op().class() == Class::LoadNameCall {
            let index = global_capture_index(self.region, instruction)?;
            let callee = self.capture(index);
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
