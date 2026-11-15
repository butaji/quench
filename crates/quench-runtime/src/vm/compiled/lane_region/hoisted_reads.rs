//! Reads a loop's preheader hoisted (`Lowering::hoist_reads`): each use defines its
//! register from the hoisted variable, with the read's facts.
use super::*;

impl Lowering<'_, '_> {
    /// `instruction`, whose value `read` holds since the loop's preheader: its register
    /// takes the held value and facts, and the receiver the facts the read proved.
    pub(super) fn hoisted_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, read: LoopRead) {
        let LoopRead { value: variable, ty, representation, receiver, header, .. } = read;
        if let Some(receiver) = receiver {
            prove_receiver(state, instruction, receiver, header);
        }
        let raw = self.builder.use_var(variable);
        if representation == Representation::Int64 {
            // An integer operation's word, evaluated once for the loop (`hoist_pure`), or advanced by its counter's steps.
            self.define(state, instruction.result_register(), raw, RegisterFact::definition(ty, Representation::Int64));
            self.native += 1;
            return;
        }
        if representation == Representation::Int32 && instruction.op().is_load_capture() {
            // Captures have no tagged field-cache consumer. Preserve the
            // admitted view; recovery or a tagged use boxes it on demand.
            self.set_int(state, instruction.result_register(), raw);
            self.native += 1;
            return;
        }
        let destination = instruction.result_register();
        let value = match instruction.field_lookup() {
            Some(lookup @ FieldLookup::Atom { .. }) if instruction.op().is_get_field() => self.hoisted_field(state, pc, lookup, read),
            _ if representation == Representation::Int32 => int_tagged(&mut self.builder, raw),
            _ => raw,
        };
        self.define(state, destination, value, RegisterFact::definition(ty, Representation::Tagged));
        if ty == Type::INT32 {
            let int = if representation == Representation::Int32 { raw } else { self.raw_unary(Unary::Narrow(Lane::I32), value) };
            self.define_int_view(state, destination, value, int);
        }
        self.native += 1;
    }

    /// The value of the field read `lookup` at `pc`, which `read` holds since the preheader: the
    /// field value the receiver's register proves from here on.
    fn hoisted_field(&mut self, state: &mut State, pc: usize, lookup: FieldLookup, read: LoopRead) -> ir::Value {
        let FieldLookup::Atom { atom, base, cache_site } = lookup else { unreachable!("a hoisted field read names an atom") };
        let exit = self.guard_exit(state, pc as u32);
        let held = HeldRead { hoisted: Some(read), ..HeldRead::default() };
        let value = self.field_value(state, (base, lookup), &[], held, exit)
            .expect("a hoisted read answers from its variable");
        #[cfg(test)]
        record_hoisted_field(cache_site, read.representation);
        #[cfg(not(test))] let _ = cache_site;
        state.proofs.fields.push(FieldValue { owner: base.register_index(), atom, value, ty: read.ty });
        value
    }
}

/// The receiver `instruction` reads takes the type, and the header, its hoisted read proved.
fn prove_receiver(state: &mut State, instruction: WideInstruction, (owner, ty): (Register, Type), header: Option<ObjectHeader>) {
    let owner = match instruction.field_lookup() {
        Some(FieldLookup::Atom { base, .. }) => base.register_index().unwrap_or(owner),
        _ => owner,
    };
    let fact = &mut state.values[usize::from(owner)];
    // An unboxed register has no tagged view to keep (`headers::object_header`).
    if fact.tagged.current() { *fact = RegisterFact { tagged: fact.tagged, ..RegisterFact::definition(ty, Representation::Tagged) }; }
    if let Some(header) = header {
        state.proofs.headers[usize::from(owner)] = Some(header);
    }
}

/// Counts the forwarded read at `cache_site`, and an int32 one at the watched site.
#[cfg(test)]
fn record_hoisted_field(cache_site: u16, representation: Representation) {
    record_field_forward(cache_site);
    if representation == Representation::Int32 {
        LOOP_INT32_READ_SITE.with(|site| {
            if let Some((expected, count)) = site.get() && expected == cache_site {
                site.set(Some((expected, count + 1)));
            }
        });
    }
}
