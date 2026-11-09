//! Reads a loop's preheader hoisted (`Lowering::hoist_reads`): each use defines its
//! register from the hoisted variable, with the read's facts.
use super::*;

impl Lowering<'_, '_> {
    /// `instruction`, whose value `read` holds since the loop's preheader: its register
    /// takes the held value and facts, and the receiver the facts the read proved.
    pub(super) fn hoisted_read(&mut self, state: &mut State, instruction: WideInstruction, read: LoopRead) {
        let LoopRead { value: variable, ty, representation, receiver, header } = read;
        if let Some(receiver) = receiver {
            prove_receiver(state, instruction, receiver, header);
        }
        let raw = self.builder.use_var(variable);
        if representation == Representation::Int32 && instruction.op().class().eq(Class::LoadCapture) {
            // Captures have no tagged field-cache consumer. Preserve the
            // admitted view; recovery or a tagged use boxes it on demand.
            self.set_int(state, instruction.result_register(), raw);
            self.native += 1;
            return;
        }
        let value = if representation == Representation::Int32 { int_tagged(&mut self.builder, raw) } else { raw };
        let destination = instruction.result_register();
        if instruction.op().class().eq(Class::GetField)
            && let Some(FieldLookup::Atom { atom, base, cache_site }) = instruction.field_lookup() {
            #[cfg(test)]
            record_hoisted_field(cache_site, representation);
            #[cfg(not(test))] let _ = cache_site;
            state.proofs.fields.push(FieldValue { owner: base.register_index(), atom, value, ty });
        }
        self.define(state, destination, value, RegisterFact::definition(ty, Representation::Tagged));
        if ty == Type::INT32 {
            let int = if representation == Representation::Int32 { raw } else { self.ins().ireduce(types::I32, value) };
            self.define_int_view(state, destination, value, int);
        }
        self.native += 1;
    }
}

/// The receiver `instruction` reads takes the type, and the header, its hoisted read proved.
fn prove_receiver(state: &mut State, instruction: WideInstruction, (owner, ty): (Register, Type), header: Option<ObjectHeader>) {
    let owner = match instruction.field_lookup() {
        Some(FieldLookup::Atom { base, .. }) => base.register_index().unwrap_or(owner),
        _ => owner,
    };
    let fact = &mut state.values[usize::from(owner)];
    *fact = RegisterFact { tagged: fact.tagged, ..RegisterFact::definition(ty, Representation::Tagged) };
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
