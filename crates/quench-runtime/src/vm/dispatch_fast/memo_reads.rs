//! Read memos: the answers a call site keeps for a callee whose body is a field read, and their hits.

use super::*;

/// Passed arguments stay in the call window; missing parameters read as absent.
#[derive(Clone, Copy)]
pub(super) struct ReadArguments {
    registers: Registers,
    base: u16,
    passed: u16,
}

impl ReadArguments {
    /// A call that passes nothing, as `[[Get]]` calls a getter: `get` reads no register.
    const NONE: Self = Self { registers: Registers(std::ptr::null_mut()), base: 0, passed: 0 };

    #[inline(always)]
    pub(super) fn get(self, parameter: u16) -> Option<Value> {
        (parameter < self.passed).then(|| self.registers.get(self.base + parameter))
    }

    /// The arguments a body of `params` parameters passes on in order.
    #[inline(always)]
    pub(super) fn within(self, params: u16) -> Self {
        Self { passed: self.passed.min(params), ..self }
    }
}

/// The hit a field record of a read body answers with: its quickened primary entry, own or
/// held by a record prototype (`lane_get_field_own`, `lane_get_field_holder`). Each hit is
/// its cache's own validated read, whatever the record's state when it runs.
#[derive(Clone, Copy)]
pub(super) enum FieldHit {
    Own(u16),
    Holder(u16),
}

/// Every bound own-field read (`own_formed_handler`), of `this` and of a register.
pub(super) fn own_bound_handlers<H: Host>() -> [*const (); 12] {
    use crate::value_vec::{EXTRAS_FORM, INLINE_FORM, NARROW_FORM};
    macro_rules! forms {
        ($($form:expr),*) => {
            [$(lane_get_field_own_bound::<H, true, { $form }> as *const (), lane_get_field_own_bound::<H, false, { $form }> as *const ()),*]
        };
    }
    forms!(HINTED_FORM, 0, EXTRAS_FORM, INLINE_FORM, EXTRAS_FORM | INLINE_FORM, NARROW_FORM)
}

impl FieldHit {
    /// The hit `record`'s quickened handler names; `None` for any other record state.
    #[inline(always)]
    pub(super) fn of<H: Host>(record: &LaneInstruction) -> Option<Self> {
        let handler = record.handler;
        if handler == lane_get_field_own::<H, true> as *const ()
            || handler == lane_get_field_own::<H, false> as *const ()
            || own_bound_handlers::<H>().contains(&handler)
        {
            Some(Self::Own(record.c))
        } else if handler == lane_get_field_holder::<H, true> as *const ()
            || handler == lane_get_field_holder::<H, false> as *const ()
        {
            Some(Self::Holder(record.c))
        } else {
            None
        }
    }

    /// The hit on `object`; `None` for a miss.
    #[inline(always)]
    pub(super) fn run<H: Host>(self, vm: &mut Vm<H>, code: &ResidualProgram, object: Value) -> Option<Value> {
        match self {
            Self::Own(cache) => vm.primary_own_field_hit(code, object, cache),
            Self::Holder(cache) => vm.primary_record_holder_field_hit(code, object, cache),
        }
    }
}

/// The read a `CallEntry::reads` body answers with, by the state its records quickened to.
#[derive(Clone, Copy)]
pub(super) enum LeafRead {
    /// Its returning `GetField` (`lane_returning` of `kind`) on the object `base` names: the
    /// kind's hit (`returning_field_hit`), or for a holder a record prototype's method read
    /// with no call.
    Field { base: u16, kind: u8, cache: u16 },
    /// Its nested own read (`nested_own_field_hit`) of field site `site`.
    Nested { site: u32 },
    /// Its field record's hit, then the dense element the parameter `index` names.
    Element { base: u16, field: FieldHit, index: u16 },
    /// Its returning private field read (`RETURNING_FIELD_PRIVATE`) of `atom` at its record
    /// `site`, under the home the callee's environment `environment` finds.
    Private { base: u16, atom: crate::bytecode::Atom, site: usize, environment: Value, function: u32 },
}

impl LeafRead {
    /// The read `target`'s records answer with now; `None` for a record state no read hits.
    ///
    /// # Safety
    /// `target` is a `CallEntry::reads` target of the running program, whose view holds its
    /// returning record first and, for an element read, its `GetIndex` record after it.
    #[inline(always)]
    pub(super) unsafe fn of<H: Host>(target: &ScriptCallTarget) -> Option<Self> {
        // SAFETY: the program store keeps a derived view alive with its program.
        let entry = unsafe { &*(*target.view).records() };
        if target.entry == CallEntry::ReadsElement {
            // SAFETY: as the function's contract.
            let element = unsafe { &*(*target.view).records().add(1) };
            return Some(Self::Element { base: entry.b, field: FieldHit::of::<H>(entry)?, index: element.c });
        }
        let handler = entry.handler;
        if handler == lane_returning::<H, RETURNING_FIELD_NESTED, NOT_LEAF> as *const () {
            return Some(Self::Nested { site: entry.imm() });
        }
        if handler == lane_returning::<H, RETURNING_FIELD_PRIVATE, NOT_LEAF> as *const () {
            return Some(Self::Private {
                base: entry.b,
                atom: entry.imm(),
                site: std::ptr::from_ref(entry).addr(),
                environment: target.env,
                function: target.function,
            });
        }
        let kind = if handler == lane_returning::<H, RETURNING_FIELD_POLYMORPHIC, NOT_LEAF> as *const () {
            RETURNING_FIELD_POLYMORPHIC
        } else if handler == lane_returning::<H, RETURNING_FIELD_OWN, NOT_LEAF> as *const () {
            RETURNING_FIELD_OWN
        } else if handler == lane_returning::<H, RETURNING_FIELD_HOLDER, NOT_LEAF> as *const () {
            RETURNING_FIELD_HOLDER
        } else if handler == lane_returning::<H, RETURNING_FIELD_INHERITED, NOT_LEAF> as *const () {
            RETURNING_FIELD_INHERITED
        } else {
            return None;
        };
        Some(Self::Field { base: entry.b, kind, cache: entry.c })
    }

    /// The read's value for a call passing `arguments` on `this`; `None` for a miss.
    #[inline(always)]
    pub(super) fn run<H: Host>(
        self,
        vm: &mut Vm<H>,
        code: &ResidualProgram,
        arguments: ReadArguments,
        this: Value,
    ) -> Option<Value> {
        let base = |base: u16| lane_read_base(crate::bytecode::FieldBase(base), arguments, this);
        match self {
            Self::Element { base: field_base, field, index } => {
                let array = field.run(vm, code, base(field_base)?)?;
                vm.direct_dense_element(array, arguments.get(index)?)
            }
            Self::Nested { site } => {
                // SAFETY: a nested record of the running program names its site.
                let site = unsafe { code.field_sites.get_unchecked(site as usize) };
                nested_own_field_hit(vm, code, site, lane_read_base(site.base, arguments, this)?)
            }
            // The callee runs frameless, capturing from its closure's environment.
            Self::Private { base: field_base, atom, site, environment, function } => {
                vm.site_private_field(code, site, (environment, function, false), base(field_base)?, atom)
            }
            Self::Field { base: field_base, kind, cache } => {
                let object = base(field_base)?;
                match kind {
                    RETURNING_FIELD_POLYMORPHIC => {
                        returning_field_hit::<H, RETURNING_FIELD_POLYMORPHIC>(vm, code, object, cache)
                    }
                    RETURNING_FIELD_OWN => returning_field_hit::<H, RETURNING_FIELD_OWN>(vm, code, object, cache),
                    // A record prototype's method, read with no call; any other declines the
                    // read, and the activation's own record reads it.
                    RETURNING_FIELD_HOLDER => vm.primary_record_holder_field_hit(code, object, cache),
                    _ => returning_field_hit::<H, RETURNING_FIELD_INHERITED>(vm, code, object, cache),
                }
            }
        }
    }
}

/// A `CallEntry::ForwardsRead` body's reads: the field of `this` or a parameter it reads,
/// then the method of that field's value it calls.
#[derive(Clone, Copy)]
pub(super) struct ForwardRead {
    base: u16,
    field: FieldHit,
    method: FieldHit,
}

impl ForwardRead {
    /// The reads `target`'s records answer with now; `None` for a record state no read hits.
    ///
    /// # Safety
    /// `target` is a `CallEntry::ForwardsRead` target of the running program, whose view
    /// holds its method read after its field read.
    #[inline(always)]
    pub(super) unsafe fn of<H: Host>(target: &ScriptCallTarget) -> Option<Self> {
        // SAFETY: the program store keeps a derived view alive with its program.
        let (field, method) = unsafe {
            let records = (*target.view).records();
            (&*records, &*records.add(1))
        };
        Some(Self { base: field.b, field: FieldHit::of::<H>(field)?, method: FieldHit::of::<H>(method)? })
    }

    /// The method the body calls and its receiver, for a call passing `arguments` on `this`.
    #[inline(always)]
    pub(super) fn method<H: Host>(
        self,
        vm: &mut Vm<H>,
        code: &ResidualProgram,
        arguments: ReadArguments,
        this: Value,
    ) -> Option<(Value, Value)> {
        let base = lane_read_base(crate::bytecode::FieldBase(self.base), arguments, this)?;
        let receiver = self.field.run(vm, code, base)?;
        Some((self.method.run(vm, code, receiver)?, receiver))
    }
}

/// A read a call site's memo makes (`MemoRead`): its field steps as copies of the field
/// cache entries that answered them (`Vm::record_field_entry_hit`), so a step is one shape
/// test. A read of a polymorphic or inherited entry, which probes further, has no memo.
#[derive(Clone, Copy)]
pub(super) enum MemoLeaf {
    /// The field of the object `base` names (`lane_read_base`).
    Field { base: u16, field: super::FieldCache },
    /// That field's array, then its dense element the parameter `index` names.
    Element { base: u16, field: super::FieldCache, index: u16 },
    /// That field's array, then its `length` (`Vm::array_length_field`).
    Length { base: u16, field: super::FieldCache },
    /// That field's object, then its own field `second`.
    Nested { base: u16, field: super::FieldCache, second: super::FieldCache },
}

/// The memo shapes `lane_script_call_memo` takes as its parameter: a leaf's step
/// (`MemoLeaf::shape`), with `MEMO_FORWARD` set for a leaf a forwarding body reaches
/// (`MemoRead::Forward`), so a site's handler makes its memo's steps with no dispatch on them.
pub(super) const MEMO_FIELD: u8 = 0;
pub(super) const MEMO_ELEMENT: u8 = 1;
pub(super) const MEMO_LENGTH: u8 = 2;
pub(super) const MEMO_NESTED: u8 = 3;
pub(super) const MEMO_FORWARD: u8 = 4;

impl MemoLeaf {
    /// Its step's memo shape.
    pub(super) fn shape(&self) -> u8 {
        match self {
            Self::Field { .. } => MEMO_FIELD,
            Self::Element { .. } => MEMO_ELEMENT,
            Self::Length { .. } => MEMO_LENGTH,
            Self::Nested { .. } => MEMO_NESTED,
        }
    }

    /// The memo form of `read`, whose hits the active program's caches just answered; `None`
    /// for a polymorphic or inherited read.
    pub(super) fn of<H: Host>(vm: &Vm<H>, code: &ResidualProgram, read: LeafRead) -> Option<Self> {
        let entry = |hit: FieldHit| match hit {
            FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
        };
        Some(match read {
            LeafRead::Field { base, kind: RETURNING_FIELD_OWN | RETURNING_FIELD_HOLDER, cache } => {
                Self::Field { base, field: vm.field_cache_entry(cache) }
            }
            LeafRead::Field { .. } => return None,
            // The memo holds for its callee alone, whose environment fixes the home, so the
            // site's shape and slot under it are a plain own field step.
            LeafRead::Private { base, site, environment, .. } => {
                Self::Field { base, field: vm.site_private_field_entry(site, environment)? }
            }
            LeafRead::Element { base, field, index } => Self::Element { base, field: entry(field), index },
            LeafRead::Nested { site } => {
                // SAFETY: a nested record of the running program names its site.
                let site = unsafe { code.field_sites.get_unchecked(site as usize) };
                let (base, field) = (site.base.0, vm.field_cache_entry(site.first.1));
                match site.second {
                    None => Self::Field { base, field },
                    Some((atom, _)) if atom == vm.length_atom => Self::Length { base, field },
                    Some((_, second)) => Self::Nested { base, field, second: vm.field_cache_entry(second) },
                }
            }
        })
    }

    /// This memo with its steps as `Vm::formed_entry` makes them of the objects a call passing
    /// `arguments` on `this` reads, which its steps just answered: each step then proves its
    /// shapes and storage forms by whole-word compares (`Vm::record_field_formed_hit`).
    pub(super) fn formed<H: Host>(self, vm: &Vm<H>, arguments: ReadArguments, this: Value) -> Self {
        let base_of = |base: u16| lane_read_base(crate::bytecode::FieldBase(base), arguments, this);
        let formed = |field: super::FieldCache, base: u16| base_of(base).map_or(field, |object| vm.formed_entry(field, object));
        match self {
            Self::Field { base, field } => Self::Field { base, field: formed(field, base) },
            Self::Element { base, field, index } => Self::Element { base, field: formed(field, base), index },
            Self::Length { base, field } => Self::Length { base, field: formed(field, base) },
            Self::Nested { base, field, second } => {
                let object = base_of(base).and_then(|object| vm.record_field_entry_hit(field, object));
                let second = object.map_or(second, |object| vm.formed_entry(second, object));
                Self::Nested { base, field: formed(field, base), second }
            }
        }
    }

    /// `LeafRead::run` of the read this memo records, of the step `shape` names (a constant
    /// where it inlines); `None` for a memo of another step.
    #[inline(always)]
    pub(super) fn run<H: Host>(
        &self,
        shape: u8,
        vm: &Vm<H>,
        arguments: ReadArguments,
        this: Value,
        this_record: Option<&crate::heap::Object>,
    ) -> Option<Value> {
        match *self {
            Self::Field { base: field_base, field } if shape == MEMO_FIELD => memo_first_step(vm, field, field_base, arguments, this, this_record),
            Self::Element { base: field_base, field, index } if shape == MEMO_ELEMENT => {
                let array = memo_first_step(vm, field, field_base, arguments, this, this_record)?;
                vm.direct_dense_element(array, arguments.get(index)?)
            }
            Self::Length { base: field_base, field } if shape == MEMO_LENGTH => {
                let array = memo_first_step(vm, field, field_base, arguments, this, this_record)?;
                vm.array_length_field(array, vm.length_atom)
            }
            Self::Nested { base: field_base, field, second } if shape == MEMO_NESTED => {
                #[cfg(feature = "profile-field-primitives")]
                vm.profile.field_primitives.hit(crate::profile::field_primitives::Path::MemoPairAttempt);
                let object = memo_first_step(vm, field, field_base, arguments, this, this_record)?;
                #[cfg(feature = "profile-field-primitives")]
                vm.profile.field_primitives.hit(crate::profile::field_primitives::Path::MemoPairFirstHit);
                let value = vm.record_field_formed_hit(second, object);
                #[cfg(feature = "profile-field-primitives")]
                if value.is_some() { vm.profile.field_primitives.hit(crate::profile::field_primitives::Path::MemoPairHit); }
                value
            }
            _ => None,
        }
    }
}

/// A memo leaf's first step (`MemoLeaf::run`): `field` of the object `base` names, read
/// from `this`'s record where the caller holds it (a forwarding body's receiver, already
/// proven a direct object).
#[inline(always)]
pub(super) fn memo_first_step<H: Host>(
    vm: &Vm<H>,
    field: super::FieldCache,
    base: u16,
    arguments: ReadArguments,
    this: Value,
    this_record: Option<&crate::heap::Object>,
) -> Option<Value> {
    match this_record {
        Some(record) if base == crate::bytecode::FieldBase::THIS.0 => vm.record_field_formed_hit_in(field, record),
        _ => vm.record_field_formed_hit(field, lane_read_base(crate::bytecode::FieldBase(base), arguments, this)?),
    }
}

/// What a call site's read target answered with (`lane_script_call_memo`), so the site's
/// next call of the same callee makes the hits alone: the target's read, or a forwarding
/// body's field and method reads and the read of the method they found. Every part depends
/// on the callee alone, whose body is fixed, and on the shapes its steps name.
#[derive(Clone, Copy)]
pub(super) enum MemoRead {
    Leaf(MemoLeaf),
    Forward {
        /// The forwarding body's field read: the object `base` names, then its method.
        base: u16,
        field: super::FieldCache,
        method_field: super::FieldCache,
        /// The forwarding body's parameters, which it passes on in order.
        params: u16,
        /// The method the reads found, a `CallEntry::reads` target, whose `this` is not lexical.
        method: Value,
        read: MemoLeaf,
    },
}

impl MemoRead {
    /// Its memo shape (`MEMO_FIELD` .. `MEMO_FORWARD`).
    pub(super) fn shape(&self) -> u8 {
        match self {
            Self::Leaf(read) => read.shape(),
            Self::Forward { read, .. } => MEMO_FORWARD | read.shape(),
        }
    }
}

/// The `lane_script_call_memo` instantiation of memo shape `shape` (`MemoRead::shape`).
pub(super) fn script_call_memo_handler<H: Host>(shape: u8) -> Handler {
    const _: () = assert!(MEMO_NESTED < MEMO_FORWARD);
    macro_rules! shapes {
        ($($shape:expr),*) => {
            match shape {
                $(shape if shape == $shape => lane_script_call_memo::<H, { $shape }> as Handler,)*
                _ => unreachable!("a memo shape is a leaf step, forwarded or not"),
            }
        };
    }
    shapes!(
        MEMO_FIELD,
        MEMO_ELEMENT,
        MEMO_LENGTH,
        MEMO_NESTED,
        MEMO_FORWARD | MEMO_FIELD,
        MEMO_FORWARD | MEMO_ELEMENT,
        MEMO_FORWARD | MEMO_LENGTH,
        MEMO_FORWARD | MEMO_NESTED
    )
}

/// A call site's memo of its read (`MemoRead`), valid for `callee` in the generation and
/// program it was made in, as `ScriptCallTarget` is.
#[derive(Clone, Copy)]
pub(super) struct ReadMemo {
    /// The call record that made it, whose slot (`CallOperand::memo`) it is.
    site: Ip,
    callee: Value,
    admission: Admission,
    this_binding: ThisBinding,
    read: MemoRead,
}

/// Slots of the read memo table (`LaneStack::read_memos`): it bounds the table's memory, and a
/// table that fills starts again empty, so the sites that read since make their memos again.
pub(super) const READ_MEMOS: usize = 4096;

/// The memo the call record `site` made, in the slot it names (`CallOperand::memo`); `None`
/// before its first, or once the table refilled and gave the slot to another site.
#[inline(always)]
pub(super) fn site_read_memo<'a>(memos: &'a [ReadMemo], site: &LaneInstruction) -> Option<&'a ReadMemo> {
    // SAFETY: a memo's site is a call record (`Op::makes_script_call`), whose operand is `call`.
    let slot = unsafe { site.operand.call.memo };
    memos.get(slot as usize).filter(|held| std::ptr::eq(held.site, site))
}

/// Records `target`'s read at the call record `site` (`lane_memoize_read`) unless the site's
/// own memo of this generation missed, which keeps the full form until the generation ends.
#[inline(always)]
pub(super) fn lane_note_read<H: Host>(
    cx: *mut LaneContext<H>,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    let stack = unsafe { &vm(cx).lane_stack };
    // SAFETY: `site` is a call record of the running view.
    if site_read_memo(&stack.read_memos, unsafe { &*site }).is_none_or(|held| held.admission.generation() != stack.generation) {
        lane_memoize_read(cx, site, target, arguments, this);
    }
}
