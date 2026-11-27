//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
use super::*;

/// The value the primary own entry of site `site` reads in `object`, in whatever storage form
/// the object keeps it: the site row (`primary_own_field_hit`) reads arena runs and packed
/// values; an object whose values a collection narrowed (`NARROW_FORM`) is read through the
/// entry formed to its word (`form_hint`), so its record settles on the narrow handler instead
/// of missing on every read.
#[inline(always)]
pub(super) fn own_entry_hit(vm: &mut Vm, code: &ResidualProgram, object: Value, site: u16) -> Option<Value> {
    if let Some(value) = vm.primary_own_field_hit(code, object, site) {
        return Some(value);
    }
    let entry = vm.field_cache_entry(site);
    let receiver = vm.heap_reads().record_object_value(object)?;
    if entry.holder != quench_runtime::vm::NO_FIELD_HOLDER || receiver.exact_shape() != entry.receiver {
        return None;
    }
    vm.own_field_hint_hit::<HINTED_FORM>(vm.form_hint(entry, object), receiver)
}

/// The native kind index a typed site fixes (`TYPED_KIND_SITES`), `None` for any other site.
pub(crate) const fn typed_site_kind(site: u8) -> Option<u32> {
    let mut index = 0;
    while index < TYPED_KIND_SITES.len() {
        if TYPED_KIND_SITES[index] == site {
            return Some(index as u32);
        }
        index += 1;
    }
    None
}

/// The typed keyed read or write row of the native kind at `index`, whose site fixes the kind.
pub(crate) const fn element_typed_row(read: bool, index: u32) -> Handler {
    macro_rules! rows {
        ($($index:literal)*) => {
            match (read, index) {
                $((true, $index) => generated_row!(ELEMENT_READ, ACC_NONE, TYPED_KIND_SITES[$index]),
                (false, $index) => generated_row!(ELEMENT_WRITE, ACC_NONE, TYPED_KIND_SITES[$index]),)*
                _ => panic!("no native typed kind at this index"),
            }
        };
    }
    rows!(0 1 2 3 4 5 6 7 8)
}

/// The typed array kind the element record at `pc` of `view` observed, where its row is a typed
/// element row (`requicken_typed`); `None` for any other record.
pub(crate) fn typed_element_kind(view: &LaneView, pc: usize) -> Option<TypedArrayKind> {
    if pc >= view.records.len() { return None; }
    // SAFETY: `pc` names a record of the view.
    let record = unsafe { &*view.records.base().add(pc) };
    (is_typed_element_row(record.handler, true) || is_typed_element_row(record.handler, false))
        // SAFETY: a typed element record's immediate is its native kind's index (`requicken_typed`).
        .then(|| quench_runtime::staged::typed_elements::native_kind_at(unsafe { record.operand.imm }).kind)
}

/// Whether `handler` is a typed keyed read row (`read`) or write row, of any kind (`element_typed_row`).
pub(crate) fn is_typed_element_row(handler: *const (), read: bool) -> bool {
    (0..TYPED_KIND_SITES.len() as u32).any(|index| element_typed_row(read, index) as *const () == handler)
}

/// The opcode of the catalog's named field store of `receiver`, strict or not (`Op::field_store`),
/// whose generated row (`generated_row`) a store record runs: its hint as a layout store.
pub(super) const fn field_store_opcode(receiver: StoreReceiver, strict: bool) -> u16 {
    let mut index = 0;
    loop {
        match Op::from_index(index) {
            Some(op) => match op.field_store() {
                Some(FieldStore { receiver: found, strict: s })
                    if s == strict && matches!((found, receiver), (StoreReceiver::This, StoreReceiver::This)
                        | (StoreReceiver::Register, StoreReceiver::Register)) => break index as u16,
                _ => index += 1,
            },
            None => panic!("the catalog names every field store"),
        }
    }
}

/// The generated own-field store (`generated_row` of the store opcode) of a store record whose
/// receiver and strictness `store` names.
pub(super) const fn field_store_row(store: FieldStore) -> Handler { field_store_row_at::<FLAG_UNREAD>(store) }

/// `field_store_row` at site fact `FLAG`: `ADD_SITE` is the row of a store that may add its property.
pub(super) const fn field_store_row_at<const FLAG: u8>(store: FieldStore) -> Handler {
    match (store.receiver, store.strict) {
        (StoreReceiver::Register, false) => generated_row!(FIELD_STORE, ACC_NONE, FLAG_UNREAD),
        (StoreReceiver::Register, true) => generated_row!(FIELD_STORE_STRICT, ACC_NONE, FLAG_UNREAD),
        (StoreReceiver::This, false) => generated_row!(THIS_FIELD_STORE, ACC_NONE, FLAG_UNREAD),
        (StoreReceiver::This, true) => generated_row!(THIS_FIELD_STORE_STRICT, ACC_NONE, FLAG_UNREAD),
    }
}

/// Whether store opcode `op` takes `this` as its receiver (`THIS_FIELD_STORE`, `THIS_FIELD_STORE_STRICT`).
pub(super) const fn this_store_op(op: u16) -> bool { op == THIS_FIELD_STORE || op == THIS_FIELD_STORE_STRICT }

/// Whether site `site` is a store's own-field instance (`store_formed_handler`), of either form and receiver.
pub(crate) const fn store_formed_site(site: u8) -> bool {
    site == own_site(false, STORE_FLAT) || site == own_site(true, STORE_FLAT)
        || site == own_site(false, STORE_INLINE) || site == own_site(true, STORE_INLINE)
}

/// The store row of opcode `OP` at the instance of the storage form `form` (`own_form_index`): the flat run's
/// (`1` untagged, `6` tagged, one instance since a store writes the same word either way) or packed inline
/// values'. Extras and narrow runs have none: their stores stay plain.
pub(super) fn store_formed_handler<const OP: u16>(form: usize) -> Option<Handler> {
    match form {
        1 | 6 => Some(if const { this_store_op(OP) } { generated_row!(OP, ACC_NONE, own_site(true, STORE_FLAT)) }
            else { generated_row!(OP, ACC_NONE, own_site(false, STORE_FLAT)) }),
        3 => Some(if const { this_store_op(OP) } { generated_row!(OP, ACC_NONE, own_site(true, STORE_INLINE)) }
            else { generated_row!(OP, ACC_NONE, own_site(false, STORE_INLINE)) }),
        _ => None,
    }
}

/// Requickens the plain own-field store of `script`'s record at `ip` to the instance of its hint's storage
/// form (`store_formed_handler`), the hint `layout` the store just matched or a VM store set. The arena that
/// keeps slot representations (`tracks_slot_representations`) leaves a store plain: its tracked site owns it.
#[inline(never)]
pub(crate) fn requicken_store_form<const OP: u16>(script: *mut LaneContext, ip: Ip, layout: u32) {
    if unsafe { vm(script) }.heap_reads().tracks_slot_representations() { return; }
    if let Some(handler) = store_formed_handler::<OP>(own_form_index(Some(layout))) { requicken(ip, handler); }
}

/// Whether site `site` is a tracked store installed for its slot's held type (`TRACKED_TAGGED_SITE` and its siblings).
pub(crate) const fn store_held_site(site: u8) -> bool {
    site == TRACKED_TAGGED_SITE || site == TRACKED_INT_SITE || site == TRACKED_OBJECT_SITE || site == TRACKED_OBJECT_OR_NULL_SITE
}

/// The held type (`HELD_*`) the tracked store site `site` is installed for; `HELD_NONE` at any other site.
pub(crate) const fn held_of(site: u8) -> u8 {
    use quench_runtime::staged::{HELD_INT32, HELD_NONE, HELD_OBJECT, HELD_OBJECT_OR_NULL, HELD_TAGGED};
    if site == TRACKED_TAGGED_SITE { HELD_TAGGED }
    else if site == TRACKED_INT_SITE { HELD_INT32 }
    else if site == TRACKED_OBJECT_SITE { HELD_OBJECT }
    else if site == TRACKED_OBJECT_OR_NULL_SITE { HELD_OBJECT_OR_NULL }
    else { HELD_NONE }
}

/// The tracked store row of opcode `OP` through the hint `(layout, slot)`: the row of the type the slot
/// holds now (`Heap::tracked_store_held`), which stores a value of it in place with no join; else the
/// tracked row (`TRACKED_SITE`), which stores through the VM and chooses again from the hint it leaves.
#[inline(never)]
pub(crate) fn tracked_store_row<const OP: u16>(script: *mut LaneContext, layout: u32, slot: u16) -> Handler {
    use quench_runtime::staged::{HELD_INT32, HELD_OBJECT, HELD_OBJECT_OR_NULL, HELD_TAGGED};
    match unsafe { vm(script) }.heap_reads().tracked_store_held(layout, slot) {
        HELD_TAGGED => generated_row!(OP, ACC_NONE, TRACKED_TAGGED_SITE),
        HELD_INT32 => generated_row!(OP, ACC_NONE, TRACKED_INT_SITE),
        HELD_OBJECT => generated_row!(OP, ACC_NONE, TRACKED_OBJECT_SITE),
        HELD_OBJECT_OR_NULL => generated_row!(OP, ACC_NONE, TRACKED_OBJECT_OR_NULL_SITE),
        _ => generated_row!(OP, ACC_NONE, TRACKED_SITE),
    }
}

/// The site fact (`generated_row`'s `FLAG`) of an own-field read of `this` (else of a register) whose
/// hint fixes form `OWN_FORMS[form]`. Zero is no site: the row reads it as unread.
pub(super) const fn own_site(this: bool, form: usize) -> u8 { 1 + (form as u8) * 2 + this as u8 }

/// Whether site `site` is a read through a prototype hint.
pub(crate) const fn held_site(site: u8) -> bool {
    site == HELD_SITE_THIS || site == HELD_SITE_REGISTER || held_other_site(site)
}

/// Whether site `site` reads `this` or a register as its record says at run time, not as a
/// site fact: a cold row kept to one instantiation for both bases.
pub(crate) const fn runtime_base(site: u8) -> bool {
    site == HELD_OTHER_THIS || site == INHERITED_OTHER_THIS || entry_site(site)
}

pub(crate) const fn own_constant_site(site: u8) -> bool { site == OWN_CONSTANT_REGISTER }

pub(crate) const fn length_site(site: u8) -> bool { site == ARRAY_LENGTH_SITE || site == STRING_LENGTH_SITE }

pub(crate) const fn host_read_site(site: u8) -> bool { site == TYPED_LENGTH_SITE || site == GEOMETRY_SITE }

pub(crate) const fn getter_site(site: u8) -> bool { site == GETTER_THIS || site == GETTER_REGISTER }

/// Whether site `site` is a nested read, `NESTED_SITE` or one of its formed instances.
pub(crate) const fn nested_site(site: u8) -> bool {
    site == NESTED_SITE || site == NESTED_FLAT_FLAT || site == NESTED_FLAT_INLINE || site == NESTED_INLINE_FLAT
        || site == NESTED_INLINE_INLINE
}

/// The storage form each hop of nested site `site` fixes, the record's hint's first: a flat run
/// (`Form::Arena`) or packed inline values (`Form::Packed`) as its instance names; `Form::Either`
/// where it names none (`NESTED_SITE`).
pub(crate) const fn nested_hop_forms(site: u8) -> [Form; 2] {
    if site == NESTED_FLAT_FLAT { [Form::Arena, Form::Arena] }
    else if site == NESTED_FLAT_INLINE { [Form::Arena, Form::Packed] }
    else if site == NESTED_INLINE_FLAT { [Form::Packed, Form::Arena] }
    else if site == NESTED_INLINE_INLINE { [Form::Packed, Form::Packed] }
    else { [Form::Either, Form::Either] }
}

/// The storage-form class of a nested hop whose layout id is `layout`: `0` a flat run, `1` packed
/// inline values; `None` for another form, whose hop keeps `NESTED_SITE`'s `Form::Either`. Reads the
/// hint's form through `own_form_index`, which folds `FLOAT_SLOTS` at this requicken.
pub(super) fn nested_hop_class(layout: u32) -> Option<usize> {
    match own_form_index(Some(layout)) {
        6 => Some(0),
        3 => Some(1),
        _ => None,
    }
}

/// The formed nested row of the classes `first` and `second` (`nested_hop_class`).
pub(super) fn nested_formed_handler(first: usize, second: usize) -> Handler {
    macro_rules! row { ($site:expr) => { generated_row!(FIELD_LEAF_READ, ACC_NONE, $site) as Handler } }
    [row!(NESTED_FLAT_FLAT), row!(NESTED_FLAT_INLINE), row!(NESTED_INLINE_FLAT), row!(NESTED_INLINE_INLINE)][first * 2 + second]
}

pub(crate) const fn general_site(site: u8) -> bool { site == GENERAL_THIS || site == GENERAL_REGISTER }

/// The generated general read of a `GetField` by its base: `GENERAL_THIS` for `this`,
/// `GENERAL_REGISTER` for a register, and `row_get_field_general` for a nested or local base.
pub(super) fn general_handler(base: u16) -> Handler {
    if base == quench_runtime::bytecode::FieldBase::THIS.0 {
        generated_row!(FIELD_LEAF_READ, ACC_NONE, GENERAL_THIS)
    } else if quench_runtime::bytecode::FieldBase(base).register_index().is_some() {
        generated_row!(FIELD_LEAF_READ, ACC_NONE, GENERAL_REGISTER)
    } else {
        row_get_field_general
    }
}

/// The VM's general read of `atom` of `object` through cache `site`, as the row's host path
/// (`generated_row`'s `GENERAL_*` answer): a register's hint answers first (`primary_record_holder_field_hit`,
/// `proved_inherited_field_hit`), then the caches' probe (`field_step_in_lane`), with `record` the
/// instruction's address for a private name's site. `None` leaves to `row_get_field_general`.
#[inline(always)]
pub(crate) fn general_field_read(context: *mut (), record: usize, register: bool, object: Value, atom: u32,
    site: u16) -> Option<Value> {
    let cx = context.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let quick = if register && vm.specialized_operations_enabled(code) {
        vm.primary_record_holder_field_hit(code, object, site).or_else(|| vm.proved_inherited_field_hit(code, object, site))
    } else {
        None
    };
    quick.or_else(|| field_step_in_lane(cx, record, code, object, atom, site))
}

/// Whether the nested site `i` reads two ordinary own entries, which `NESTED_SITE`'s row runs; a
/// `length` step (an array's or a string's own `length`) or a single step stays with
/// `get_field_nested_miss`.
pub(super) fn nested_row_site(vm: &Vm, code: &ResidualProgram, i: &LaneInstruction) -> bool {
    code.field_sites.get(i.imm() as usize).is_some_and(|site| site.second.is_some_and(|(atom, _)| atom != vm.length_atom))
}

/// Whether site `site` is a field read of a Math call's callee (`MATH_CALL_SITE`, `MATH_CALL2_SITE`,
/// `MATH_CALL2_CONST_SITE`).
pub(crate) const fn math_site(site: u8) -> bool {
    site == MATH_CALL_SITE || site == MATH_CALL2_SITE || site == MATH_CALL2_CONST_SITE
}

/// The number of arguments the Math call of math site `site` passes.
pub(crate) const fn math_arity(site: u8) -> usize {
    if site == MATH_CALL_SITE { 1 } else { 2 }
}

/// Whether site `site` is a sinking read (`SINK_SITE`).
pub(crate) const fn sink_site(site: u8) -> bool { site == SINK_SITE }

/// The store `(atom, cache)` of the sinking read `i` (`SINK_SITE`): its site's sink.
#[inline(always)]
pub(crate) fn sink_operands(cx: *mut LaneContext, i: &LaneInstruction) -> Option<(quench_runtime::bytecode::Atom, u16)> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    code.field_sites.get(i.imm() as usize)?.sink
}

/// Whether site `site` reads through a VM entry lookup (`FieldPlan::Entry`, `FieldPlan::Hinted`).
pub(crate) const fn entry_site(site: u8) -> bool {
    site == POLYMORPHIC_SITE || site == HASHED_SITE || site == HOLDER_SITE || site == INHERITED_WALK_SITE
}

/// The entry lookup a non-polymorphic entry site makes (`EntryKind`); the polymorphic site's own
/// plan is `FieldPlan::Hinted`, whose entry is `EntryKind::Polymorphic` too.
pub(crate) const fn entry_kind(site: u8) -> quench_runtime::staged::property::EntryKind {
    use quench_runtime::staged::property::EntryKind;
    if site == POLYMORPHIC_SITE { EntryKind::Polymorphic }
    else if site == HASHED_SITE { EntryKind::Hashed }
    else if site == HOLDER_SITE { EntryKind::Holder }
    else { EntryKind::Inherited }
}

pub(crate) const fn private_field_site(site: u8) -> bool {
    site == PRIVATE_FIELD_THIS || site == PRIVATE_FIELD_REGISTER
}

pub(crate) const fn cached_field_site(site: u8) -> bool {
    site == CACHED_FIELD_THIS || site == CACHED_FIELD_REGISTER
}

pub(crate) const fn returning_site(this: bool, leaf: u8) -> u8 { own_site(this, RETURNING_FORM) + 2 * leaf }

pub(crate) const fn returning_field_site(site: u8) -> bool { site >= RETURNING_BASE }

/// Saturating: a const branch for a site of another family still instantiates the call.
pub(crate) const fn returning_leaf(site: u8) -> u8 { site.saturating_sub(RETURNING_BASE) / 2 }

/// Whether `handler` is the returning own-field row of no leaf form, at `this` or a register
/// (`returning_site`): the leaf call's reads recognise it as the own read it is.
pub(super) fn returning_own_row(handler: *const ()) -> bool {
    handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(true, NOT_LEAF)) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(false, NOT_LEAF)) as *const ()
}

pub(crate) const fn cell_own_site(site: u8) -> bool {
    site == CELL_OWN_THIS || site == CELL_OWN_REGISTER || site == CELL_RUN_THIS || site == CELL_RUN_REGISTER
        || site == CELL_FLOAT_RUN_THIS || site == CELL_FLOAT_RUN_REGISTER || site == CELL_FLOAT_EXTRAS_THIS || site == CELL_FLOAT_EXTRAS_REGISTER
        || site == CELL_EXTRAS_THIS || site == CELL_EXTRAS_REGISTER
}

/// Whether site `site` reads through the record's inherited hint; of any receiver (`other`).
pub(crate) const fn inherited_site(site: u8) -> bool {
    site == INHERITED_THIS || site == INHERITED_REGISTER || inherited_other_site(site)
}

pub(crate) const fn inherited_other_site(site: u8) -> bool { site == INHERITED_OTHER_THIS || site == INHERITED_OTHER_REGISTER }

/// Whether held site `site` reads any receiver's object part, not a direct object's alone.
pub(crate) const fn held_other_site(site: u8) -> bool {
    site == HELD_OTHER_THIS || site == HELD_OTHER_REGISTER || site == HELD_CELL_REGISTER
}

/// Whether own-field or held site `site` reads `this`.
pub(crate) const fn own_site_this(site: u8) -> bool { site.wrapping_sub(1) & 1 == 1 }

/// The storage form own-field site `site` fixes (`OWN_FORMS`); `HINTED_FORM` for none.
pub(crate) const fn own_site_form(site: u8) -> u32 {
    let index = (site.wrapping_sub(1) / 2) as usize;
    if index < OWN_FORMS.len() { OWN_FORMS[index] }
    else if index == OWN_FORMS.len() + 7 { quench_runtime::staged::TAGGED_SLOTS }
    else if index == OWN_FORMS.len() + 31 { 0 }
    else if index == OWN_FORMS.len() + 32 { quench_runtime::value_vec::EXTRAS_FORM }
    else if index == OWN_FORMS.len() + 33 { quench_runtime::value_vec::EXTRAS_FORM | quench_runtime::staged::TAGGED_SLOTS }
    else if index == STORE_FLAT { 0 }
    else if index == STORE_INLINE { quench_runtime::value_vec::INLINE_FORM }
    else { HINTED_FORM }
}

/// Whether the word own-field site `site` reads is a slot word to decode (`decode_staged_word`):
/// every form but a fixed tagged one and packed or narrow values, which hold no raw double.
pub(crate) const fn own_site_decodes(site: u8) -> bool {
    // A formed nested read's hops are flat runs of tagged words or packed values (`nested_hop_class`).
    if nested_site(site) && site != NESTED_SITE { return false; }
    if held_site(site) || site == STRING_SITE || site == NUMBER_SITE || inherited_site(site) || own_constant_site(site) || length_site(site)
        || host_read_site(site) || private_field_site(site) || cached_field_site(site) { return false; }
    let form = own_site_form(site);
    form == HINTED_FORM || (form & quench_runtime::staged::TAGGED_SLOTS == 0 && form != quench_runtime::value_vec::INLINE_FORM
        && form != quench_runtime::value_vec::NARROW_FORM)
}

/// The value of slot word `word` an own-field read at a decoding site took: a fixed untagged form holds raw
/// doubles (`own_formed_handler`), decoded untested; an unfixed form tests (`decode_staged_word`).
#[inline(always)]
pub(crate) fn own_site_decode(site: u8, word: u64) -> Value {
    if own_site_form(site) == HINTED_FORM { quench_runtime::value_vec::decode_staged_word(word) } else { quench_runtime::value_vec::decode_float_word(word) }
}

/// The `OWN_FORMS` index of the storage form layout id `hint` names (`0`, the hinted form, for none).
/// A narrow run's hint names neither inline values nor extras, so those tests fold away; packed and
/// narrow slots never hold a raw double, and the other forms skip the decode where float slots are off.
pub(super) fn own_form_index(hint: Option<u32>) -> usize {
    use quench_runtime::value_vec::{EXTRAS_FORM, INLINE_FORM, NARROW_FORM};
    let tagged = !quench_runtime::vm::switches::float_slots();
    // The hint is a layout id; its form bits are its entry's (`layouts::auxiliary`).
    match hint.map(quench_runtime::tier_surface::layouts::auxiliary) {
        None => 0,
        Some(hint) if hint & NARROW_FORM != 0 => 5,
        Some(hint) => match (hint & (INLINE_FORM | EXTRAS_FORM), tagged) {
            (0, true) => 6,
            (0, false) => 1,
            (EXTRAS_FORM, true) => 7,
            (EXTRAS_FORM, false) => 2,
            (INLINE_FORM, _) => 3,
            (_, true) => 8,
            (_, false) => 4,
        },
    }
}

/// The generated own-field read (`generated_row` at an `own_site`) for the base `THIS` names and the
/// storage form the layout id `hint`'s form bits name (`own_form_index`): an arena run, extras or packed
/// inline values, whose layout it then reads untested; `None` for the form of no particular hint.
pub(super) fn own_formed_handler<const THIS: bool>(hint: Option<u32>) -> Handler {
    macro_rules! row { ($site:expr) => { generated_row!(FIELD_LEAF_READ, ACC_NONE, $site) as Handler } }
    own_sites!(row)[own_form_index(hint) * 2 + usize::from(!THIS)]
}

/// Makes the own field entry `entry` the hint of the field read/store record at `ip`
/// (`LaneInstruction::field_hint`). A held loop header's `call_link` names its site
/// (`header_site_in_lane`), and such a record keeps no hint: an empty hint's link is
/// `NO_CALL_LINK`, a held one's shape a recorded one.
#[inline(always)]
pub(super) fn set_field_hint(ip: Ip, entry: quench_runtime::vm::FieldCache) {
    set_record_hint(ip, entry.receiver, entry.slot);
}

/// Makes inherited entry `index` the hint of the `GetField` record at `ip`
/// (`Vm::hinted_inherited_field_hit`), where `call_link` can name it.
#[inline(always)]
pub(super) fn set_inherited_hint(ip: Ip, index: u32) {
    if let Ok(index) = u16::try_from(index)
        && index != NO_CALL_LINK
    {
        set_record_hint(ip, INHERITED_HINT, index);
    }
}

/// Makes the prototype's answer `hold` keeps (`Vm::hold_holder_hint`, `Vm::hold_string_hint`)
/// the hint of the `GetField` record at `ip`, in place of the one it holds, which `hold` takes.
/// Out of line, so the reads that keep their hint keep their registers.
#[cold]
#[inline(never)]
pub(super) fn set_prototype_hint(ip: Ip, hold: impl FnOnce(Option<u16>) -> Option<u16>) {
    if holds_loop_site(ip) {
        return;
    }
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let owned = (unsafe { i.operand.field.shape } == HOLDER_HINT).then(|| i.call_link.get());
    if let Some(index) = hold(owned) {
        set_record_hint(ip, HOLDER_HINT, index);
    }
}

/// Writes a field record's hint, `shape` and `link`, unless it is a held loop header
/// (`holds_loop_site`).
#[inline(always)]
pub(super) fn set_record_hint(ip: Ip, shape: u32, link: u16) {
    if !holds_loop_site(ip) {
        // SAFETY: as `requicken`; `ip` is a field read/store record of a live view.
        unsafe {
            (*ip.cast_mut()).operand.field.shape = shape;
            (*ip).call_link.set(link);
        }
    }
}

/// Whether the field record at `ip` is a held loop header's, whose `call_link` names its
/// site (`header_site_in_lane`), so it keeps no hint: an empty hint's link is `NO_CALL_LINK`,
/// and a held one's shape a recorded one, `INHERITED_HINT` or `HOLDER_HINT`.
#[inline(always)]
pub(super) fn holds_loop_site(ip: Ip) -> bool {
    // SAFETY: `ip` is a field read/store record of a live view.
    let i = unsafe { &*ip };
    (unsafe { i.operand.field.shape }) == quench_runtime::vm::NO_FIELD_RECEIVER && i.call_link.get() != NO_CALL_LINK
}

/// The polymorphic read of `object` by the `GetField` record at `ip` (`RETURNING_FIELD_POLYMORPHIC`):
/// its hint, the last own entry that answered it, first, then the site's inline entries, whose own
/// hit becomes the hint; with whether a prototype's entry answered, which no hint holds.
#[inline(always)]
pub(super) fn polymorphic_read(vm: &mut Vm, code: &ResidualProgram, ip: Ip, object: Value) -> Option<(Value, bool)> {
    use quench_runtime::vm::object::PolymorphicHit;
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let (value, hit) = vm.hinted_polymorphic_field_hit(code, object, i.c, i.field_hint())?;
    if let PolymorphicHit::Own(entry) = hit {
        set_field_hint(ip, vm.form_hint(entry, object));
    }
    Some((value, matches!(hit, PolymorphicHit::Holder(_))))
}

/// The polymorphic read of `object` by the `GetField` record at `ip` (`FieldPlan::Hinted`,
/// `POLYMORPHIC_SITE`), past the record's own hint: the site's inline entries, own or of the
/// immediate prototype (`hinted_polymorphic_field_hit`). An own hit becomes the record's hint; a
/// prototype's hit holds the prototype hint (`hold_holder_hint`), kept while its watch is live, and
/// a record whose hint is a prototype's first tests that hint (`holder_hint_hit`).
#[inline]
pub(super) fn polymorphic_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    use quench_runtime::vm::object::PolymorphicHit;
    // SAFETY: the context's program outlives the lane run; `ip` is a live `GetField` record.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    let i = unsafe { &*ip };
    let holds_holder = unsafe { i.operand.field.shape } == HOLDER_HINT;
    if holds_holder && let Some(value) = vm.holder_hint_hit(i.call_link.get(), i.imm(), object) {
        return Some(value);
    }
    // The other receivers' prototype hints, held in the record's `mode` (`hold_holder_arm`).
    if let Some(value) = vm.holder_arms_hit(i.mode, i.imm(), object) {
        return Some(value);
    }
    let (value, hit) = vm.hinted_polymorphic_field_hit(code, object, i.c, i.field_hint())?;
    match hit {
        PolymorphicHit::Own(entry) => {
            if !holds_holder {
                set_field_hint(ip, vm.form_hint(entry, object));
            }
        }
        PolymorphicHit::Holder(_) => {
            // A live hint another receiver of this site answers stays: rewriting it at every
            // receiver switch re-marked and re-watched the prototype each time (V8-v7 Richards:
            // 0.71M rewrites, 5% of its run).
            if !(holds_holder && vm.holder_hint_live(i.call_link.get())) {
                set_prototype_hint(ip, |owned| {
                    if owned.is_some_and(|index| vm.holder_hint_live(index)) {
                        return None;
                    }
                    vm.hold_holder_hint(owned, i.imm(), object, value, NO_CALL_LINK)
                });
            } else {
                hold_holder_arm(vm, ip, object, value);
            }
        }
        PolymorphicHit::Hint => {}
    }
    Some(value)
}

/// Holds the prototype's answer `value` for `object`, a receiver the record's own prototype hint
/// does not name, as one of the up to four arms of the polymorphic record at `ip` (its `mode`,
/// sixteen bits each, a hint index plus one): an empty arm or one whose watch retired takes it;
/// with all four live it holds none. Out of line, as `set_prototype_hint`.
#[cold]
#[inline(never)]
pub(super) fn hold_holder_arm(vm: &mut Vm, ip: Ip, object: Value, value: Value) {
    if holds_loop_site(ip) {
        return;
    }
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let mode = i.mode;
    let Some(k) = (0..4).find(|k| {
        let arm = (mode >> (16 * k)) as u16;
        arm == 0 || !vm.holder_hint_live(arm - 1)
    }) else {
        return;
    };
    let arm = (mode >> (16 * k)) as u16;
    if let Some(index) = vm.hold_holder_hint((arm != 0).then(|| arm - 1), i.imm(), object, value, NO_CALL_LINK - 1) {
        let mode = (mode & !(0xffff << (16 * k))) | u64::from(index + 1) << (16 * k);
        // SAFETY: as `set_record_hint`; the polymorphic row reads `mode` only as its arms.
        unsafe { (*ip.cast_mut()).mode = mode };
    }
}

/// The hashed entries of the `GetField` record at `ip` (`HASHED_SITE`): the site's hashed entries
/// past the inline ones, own or of the immediate prototype (`Vm::polymorphic_field_hit`), as an AST
/// walk reads `type` of a node of any of dozens of shapes. Any other read takes `lane_get_field`.
pub(super) fn hashed_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run; `ip` is a live `GetField` record.
    let code = unsafe { &*(*cx).code };
    let i = unsafe { &*ip };
    unsafe { vm(cx) }.polymorphic_field_hit(code, object, i.c)
}

/// The site's primary holder entry of `object` (`HOLDER_SITE`): a direct object's record
/// (`Vm::primary_holder_field_hit`), else a receiver any heap cell holds, such as a function whose
/// prototype holds the method (`Vm::cell_holder_field_hit`), as `f.apply`. Holds no hint.
pub(super) fn holder_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    // SAFETY: as `hashed_entry`.
    let code = unsafe { &*(*cx).code };
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    if object.direct_object_index().is_some() {
        vm.primary_holder_field_hit(code, object, i.c)
    } else {
        vm.cell_holder_field_hit(code, object, i.c)
    }
}

/// The inherited entry of `object` walked past its immediate prototype (`INHERITED_WALK_SITE`):
/// the site's entries and their chain walk, which proves one again (`Vm::inherited_field_entry_hit`);
/// the entry that answered becomes the record's inherited hint.
pub(super) fn inherited_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    // SAFETY: as `hashed_entry`.
    let code = unsafe { &*(*cx).code };
    let i = unsafe { &*ip };
    let (value, entry) = unsafe { vm(cx) }.inherited_field_entry_hit(code, object, i.c)?;
    set_inherited_hint(ip, entry);
    Some(value)
}

/// The intrinsic geometry getter's value of `atom` on `base` (`Vm::typed_array_geometry_read`)
/// for a geometry name (`AtomClass::VIRTUAL_FIELD`); `None` for any other read.
#[inline(always)]
pub(super) fn geometry_read_in_lane(vm: &mut Vm, base: Value, atom: quench_runtime::bytecode::Atom) -> Option<Value> {
    if !vm.atom_class(atom).contains(quench_runtime::vm::object::AtomClass::VIRTUAL_FIELD) {
        return None;
    }
    vm.typed_array_geometry_read(base, atom)
}

/// The `length` of a receiver that is no array or string (`FieldPlan::Length`): an arguments
/// object's or a user function's own data `length`, else a typed view's intrinsic geometry
/// (`Vm::typed_array_geometry_read_inline`); `None` for any other receiver.
#[inline(always)]
pub(super) fn length_read(vm: &mut Vm, receiver: Value) -> Option<Value> {
    let atom = vm.length_atom;
    vm.arguments_object_length(receiver)
        .or_else(|| vm.plain_function_length(receiver))
        .or_else(|| vm.typed_array_geometry_read_inline(receiver, atom))
}

pub(super) fn field_site_tier_in_lane(
    vm: &Vm,
    code: &ResidualProgram,
    i: &LaneInstruction,
) -> FieldSiteTier {
    if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        // A step that reads `name` or `length` may prepare prototype lookup
        // (`Vm::reads_function_info`); only an array's own `length` needs no preparing.
        let nested_own = code.field_sites.get(i.imm() as usize).is_some_and(|site| {
            !vm.reads_function_info(site.first.0)
                && site.second.is_none_or(|(atom, _)| {
                    atom == vm.length_atom || !vm.reads_function_info(atom)
                })
        });
        if nested_own { FieldSiteTier::Nested } else { FieldSiteTier::General }
    } else {
        // The entry tiers come from one cache index (`Vm::field_site_tier`), not one lookup per
        // predicate, since a held or inherited local read classifies its site on every read.
        vm.field_site_tier(i.c)
    }
}

/// A nested `GetField`'s value (`this.a.b`, `r.a.b`), each step one shape compare and one
/// load where it is monomorphic: the first step through the record's own-field hint
/// (`lane_nested_hinted_first`) and the second through its site's primary own entry or an
/// array's or a string's own `length` (`nested_last_step`). A second step past its primary
/// entry is read alone from the first's value (`lane_nested_second_value`), and a first step
/// past the hint takes both steps' polymorphic entries (`lane_nested_polymorphic_value`),
/// which refreshes the hint. `None` leaves the read to the general one. The site reads
/// neither `name` nor another `length` (`lane_field_site_tier`).
///
/// # Safety
/// `cx` is the running lane's context and `i` the nested record at `ip`.
#[inline(always)]
pub(super) unsafe fn lane_nested_value(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run; a nested record names its site.
    let code = unsafe { &*(*cx).code };
    let site = unsafe { code.field_sites.get_unchecked(i.imm() as usize) };
    match unsafe { lane_nested_hinted_first(cx, r, i, site) } {
        Some(first) => {
            let vm = unsafe { vm(cx) };
            nested_last_step(vm, code, site, first).or_else(|| lane_nested_second_value(vm, code, site, first))
        }
        None => unsafe { nested_polymorphic_value_in_lane(cx, r, ip) },
    }
}

/// The first step of the nested read `site` of record `i`: the own field its own-field hint
/// (`LaneInstruction::field_hint`) names, which holds the receiver shape that step last met
/// however many its site has seen (a parser's `this` gains a field mid-parse); `None` for a
/// receiver of another shape.
///
/// # Safety
/// `cx` is the running lane's context and `i` a nested record of it naming `site`.
#[inline(always)]
pub(super) unsafe fn lane_nested_hinted_first(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
    site: &quench_runtime::bytecode::FieldSite,
) -> Option<Value> {
    let base = match site.base.register_index() {
        Some(register) => r.get(register),
        None => unsafe { fixed_this_in_lane(cx, r) }?,
    };
    (unsafe { vm(cx) }).record_field_entry_hit(i.field_hint(), base)
}

/// The second field step of the nested read `site` from `first`, the value its hinted first
/// step read, past the step's primary own entry (`nested_last_step`): the megamorphic stub
/// cache, then the step's polymorphic entries (`Vm::megamorphic_field_hit`), as `node.left.type`
/// reads nodes of many shapes behind one receiver's field. The first step is not read again.
/// Out of line, so the hinted probe keeps its registers; `None` for a `length` step or a miss.
#[inline(never)]
pub(super) fn lane_nested_second_value(
    vm: &mut Vm,
    code: &ResidualProgram,
    site: &quench_runtime::bytecode::FieldSite,
    first: Value,
) -> Option<Value> {
    match site.second {
        Some((atom, cache)) if atom != vm.length_atom => vm.megamorphic_field_hit(code, first, atom, cache),
        _ => None,
    }
}

/// `lane_nested_value` past the record's hint: each step's site's entries, the primary own
/// one, else the stub cache's and the polymorphic ones, inline or hashed
/// (`Vm::megamorphic_field_entry_hit`), as an AST walk's `node.loc.start` reads nodes of many
/// shapes, and a string's own `length`, as a tokenizer's `this.input.length` reads (`lane_cached_field`); none runs guest code. The own
/// entry that answered the first step becomes the record's hint. Out of line, so the hinted
/// probe keeps its registers; `None` leaves the read to `lane_get_field`.
///
/// # Safety
/// As for `lane_nested_value`.
#[inline(never)]
pub(super) unsafe fn nested_polymorphic_value_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run; a nested record names its site.
    let code = unsafe { &*(*cx).code };
    let site = unsafe { code.field_sites.get_unchecked((*ip).imm() as usize) };
    let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
    let vm = unsafe { vm(cx) };
    let (first, own) = match vm.primary_own_field_hit(code, base, site.first.1) {
        Some(value) => (value, Some(vm.field_cache_entry(site.first.1))),
        None => match vm.megamorphic_field_entry_hit(code, base, site.first.0, site.first.1)? {
            (value, quench_runtime::vm::object::PolymorphicHit::Own(entry)) => (value, Some(entry)),
            (value, _) => (value, None),
        },
    };
    if let Some(entry) = own {
        set_field_hint(ip, entry);
    }
    match site.second {
        None => Some(first),
        Some((atom, _)) if atom == vm.length_atom => own_length(vm, first),
        Some((atom, cache)) => vm
            .primary_own_field_hit(code, first, cache)
            .or_else(|| vm.megamorphic_field_hit(code, first, atom, cache)),
    }
}

/// A nested site's read of `base` where both steps hit primary own entries
/// (`lane_nested_value`), which runs no guest code.
#[inline(always)]
pub(crate) fn nested_own_field_hit(
    vm: &mut Vm,
    code: &ResidualProgram,
    site: &quench_runtime::bytecode::FieldSite,
    base: Value,
) -> Option<Value> {
    // Two ordinary field reads are one chain row, which proves the tables once
    // (`Vm::primary_own_chain_hit`); a single read or a `length` step is `nested_last_step`'s.
    let Some((_, second)) = site.second.filter(|(atom, _)| *atom != vm.length_atom) else {
        let first = vm.primary_own_field_hit(code, base, site.first.1)?;
        return nested_last_step(vm, code, site, first);
    };
    let value = vm.primary_own_chain_hit(code, base, (site.first.1, second));
    #[cfg(feature = "profile-field-primitives")]
    pair_census(vm, code, (site.first.1, base), value.is_some());
    value
}

/// The census of a nested pair read: its attempt, whether its first step hit (a miss of the chain
/// reads the first step alone to attribute it) and whether both did.
#[cfg(feature = "profile-field-primitives")]
pub(super) fn pair_census(vm: &mut Vm, code: &ResidualProgram, (first, base): (u16, Value), hit: bool) {
    use quench_runtime::profile::field_primitives::Path;
    vm.profile.field_primitives.hit(Path::NestedOwnPairAttempt);
    if hit || vm.primary_own_field_hit(code, base, first).is_some() {
        vm.profile.field_primitives.hit(Path::NestedOwnPairFirstHit);
    }
    if hit {
        vm.profile.field_primitives.hit(Path::NestedOwnPairHit);
    }
}

/// A nested site's last step from `first`, the first step's value: none, an array's or a
/// string's own `length`, or the second step's primary own entry.
#[inline(always)]
pub(super) fn nested_last_step(
    vm: &mut Vm,
    code: &ResidualProgram,
    site: &quench_runtime::bytecode::FieldSite,
    first: Value,
) -> Option<Value> {
    match site.second {
        None => Some(first),
        Some((atom, _)) if atom == vm.length_atom => own_length(vm, first),
        Some((_, cache)) => {
            let value = vm.primary_own_field_hit(code, first, cache);
            #[cfg(feature = "profile-field-primitives")]
            if value.is_some() { vm.profile.field_primitives.hit(quench_runtime::profile::field_primitives::Path::NestedOwnPairHit); }
            value
        }
    }
}

/// The own `length` of an array or a string, which no prototype shadows, as a nested
/// read's last step reads it (`nested_own_field_hit`, `nested_polymorphic_value_in_lane`).
#[inline(always)]
pub(super) fn own_length(vm: &Vm, object: Value) -> Option<Value> {
    // The staged row answers either by the value's tag; a form it leaves is read through the
    // string's units, or the array's record.
    vm.length_value(object).or_else(|| match vm.heap_reads().string(object) {
        Some(text) => Some(Value::length(text.len())),
        None => vm.array_length_field(object, vm.length_atom),
    })
}

/// Requickens the nested read at `ip` to the formed instance its two hops' layouts name: the record's hint
/// is the first hop's layout, and the second hop's is the layout of the intermediate object the first step
/// reads (`form_hint`'s, which the site's entry names by shape). A hop of another form, or a hop the
/// record does not hold, leaves the generic row.
///
/// # Safety
/// `cx` is the running lane's context and `i` a nested record of it naming its site, which `lane_nested_value`
/// has just read.
pub(super) unsafe fn requicken_nested(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) {
    // SAFETY: the context's program outlives the lane run; a nested record names its site.
    let code = unsafe { &*(*cx).code };
    let site = unsafe { code.field_sites.get_unchecked(i.imm() as usize) };
    let Some((_, cache)) = site.second else { return };
    let Some(first) = (unsafe { lane_nested_hinted_first(cx, r, i, site) }) else { return };
    let machine = unsafe { vm(cx) };
    let entry = machine.field_cache_entry(cache);
    let hint = i.field_hint();
    let Some(record) = machine.heap_reads().record_object_value(first) else { return };
    if hint.receiver == quench_runtime::vm::NO_FIELD_RECEIVER || entry.holder != quench_runtime::vm::NO_FIELD_HOLDER
        || entry.receiver != record.exact_shape() {
        return;
    }
    // Each hop's class is its object's layout's (a flat run of tagged words or packed inline values,
    // `nested_hop_class`), whose words the row then reads with no decode (`own_site_decodes`).
    let Some(base) = (unsafe { field_base_in_lane(cx, r, site.base) }) else { return };
    let Some(base_record) = machine.heap_reads().record_object_value(base) else { return };
    let second_layout = record.layout_id();
    let (Some(first_class), Some(second_class)) = (nested_hop_class(base_record.layout_id()), nested_hop_class(second_layout)) else { return };
    // A loop header keeps its site in the record, and a header's `mode` is the loop's: it keeps the generic row.
    if holds_loop_site(ip) { return; }
    // The formed row reads its second hop and base from the record (`nested_mode`), not from the site
    // and the cache: the hops' guards are the check, so a stale word only misses.
    // SAFETY: as `requicken`; an unfused field record's `mode` is zero and no fusion names it.
    unsafe { (*ip.cast_mut()).mode = nested_mode(second_layout, entry.slot, site.base) };
    requicken(ip, nested_formed_handler(first_class, second_class));
}

/// The `mode` word of a formed nested record: the intermediate object's layout id (bits 0..32) and its slot
/// (32..48), and the site's base (48..64), which the formed row reads with no site or cache lookup.
pub(super) const fn nested_mode(layout: u32, slot: u16, base: quench_runtime::bytecode::FieldBase) -> u64 {
    layout as u64 | (slot as u64) << 32 | (base.0 as u64) << 48
}

/// The second hop and base `nested_mode` packed.
#[inline(always)]
pub(crate) const fn nested_mode_parts(mode: u64) -> (u32, u16, quench_runtime::bytecode::FieldBase) {
    (mode as u32, (mode >> 32) as u16, quench_runtime::bytecode::FieldBase((mode >> 48) as u16))
}

/// The base of a `GetField` of a register or `this` that a frameless activation makes in
/// place (`row_get_field_beside_activations`): a public name whose site caches no getter,
/// which `getter_activation_in_lane` would run as an activation. `None` for a frame's run, a
/// nested or local base, a private name or such a getter.
#[inline(never)]
pub(super) fn field_read_beside_activations_in_lane(cx: *mut LaneContext, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !vm.lane_stack.has_frames() || i.b == quench_runtime::bytecode::FieldBase::NESTED || vm.is_private_name(i.imm()) {
        return None;
    }
    let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }?;
    vm.cached_field_getter(base, i.imm(), vm.field_cache_index(i.c)).is_none().then_some(base)
}

/// A lane `GetField` whose read meets the accessor its site cached
/// (`Vm::cached_field_getter`): the getter runs as a frameless activation of the read's
/// record, as `script_call_prove_miss` runs a call, with the base as receiver and no arguments,
/// and its completion is the read's value. `Err` carries the record to continue at: the
/// general path for any other read.
#[inline(never)]
pub(super) fn getter_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Ip> {
    // No admission test here: the getter's target is found through the admission word
    // (`lane_script_target_in`, `cross_call_entry`), which refuses a run that calls no script
    // functions or has a construction pending, and a native getter asks it in `read_getter`.
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The object the getter's read is of and its site: a nested site's second field is read
    // of the value its first, cached read gives.
    let read = if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        code.field_sites.get(i.imm() as usize).copied().and_then(|site| {
            let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
            match site.second {
                Some((atom, cache)) => {
                    Some((lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, base, site.first.0, site.first.1, 0)?, atom, cache))
                }
                None => Some((base, site.first.0, site.first.1)),
            }
        })
    } else {
        unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }.map(|base| (base, i.imm(), i.c))
    };
    let vm = unsafe { vm(cx) };
    #[cfg(feature = "profile-lane-exits")]
    if let Some((base, atom, cache)) = read
        && vm.cached_field_getter(base, atom, vm.field_cache_index(cache)).is_none()
    {
        quench_runtime::vm::lane_exits::record_detail(format!(
            "getter decline uncached {} recorded={}",
            &code.atoms[atom as usize],
            vm.field_site_getter(cache)
        ));
    } else if read.is_none() {
        quench_runtime::vm::lane_exits::record_detail("getter decline base".into());
    }
    let Some((base, getter)) = read.and_then(|(base, atom, cache)| {
        Some((base, vm.cached_field_getter(base, atom, vm.field_cache_index(cache))?))
    }) else {
        return AccessorCall::of(cx).read_get_trap(r, ip, read.map(|(base, atom, _)| (base, atom)));
    };
    AccessorCall::of(cx).read_getter(r, ip, i, getter, base)
}

/// The activation of `getter` with receiver `base` and no arguments for the read record at
/// `ip`, whose completion is the read's value (`resolved`: its frameless target, which the read
/// resolved already): a frameless target of the running program, or
/// another program's plain activation (`Vm::cross_call_entry`), which runs in its program.
/// `Err` carries the record to continue at: the general path for any other getter.
pub(super) fn activate_getter_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    getter: Value,
    base: Value,
    resolved: Option<ScriptCallTarget>,
) -> Result<(Registers, Ip), Ip> {
    // The getter's activation is pushed above the reading function's, and resolving its
    // target may displace the cached target a running leaf names, so a leaf's read makes the
    // leaf an activation first. A target the read resolved (`resolved`) is a copy, which
    // that leaves as it was.
    materialize_running_leaf_in_lane(cx);
    let vm = unsafe { vm(cx) };
    if let Some(target) = resolved.or_else(|| script_target_in_lane(cx, getter)).filter(|target| !target.forwards()) {
        // A getter is an ordinary call: the one call row where it is plain, else the kept activation.
        if let Some(entered) = getters::invoke_getter(cx, r, ip, &target, base) {
            return Ok(entered);
        }
        let Some(this) = call_this_in_lane(vm, &target, base) else {
            return Err(exit(ip));
        };
        return activate_kept_in_lane::<false>(cx, r, ip, &target, &[], this);
    }
    // A getter that makes closures, of this program or another, runs as the environment
    // activation its plain call makes (`activate_environment_in_lane`), with no arguments, as
    // Zod's `get error() { ... issues.map((iss) => ...) }` does.
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let environment = vm
        .script_call_entry(getter)
        .or_else(|| vm.cross_call_entry(getter))
        .map(|target| unsafe { *target })
        .filter(|target| target.entry == CallEntry::Environment && !constructing_in_lane(vm, getter));
    if let Some(target) = environment {
        let Some(this) = call_this_in_lane(vm, &target, base) else {
            return Err(exit(ip));
        };
        return activate_environment_in_lane(cx, r, ip, &target, &[], (this, false));
    }
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let Some(target) = vm
        .cross_call_entry(getter)
        .map(|target| unsafe { *target })
        .filter(|target| target.entry.activates() && !constructing_in_lane(vm, getter))
    else {
        #[cfg(feature = "profile-lane-exits")]
        quench_runtime::vm::lane_exits::record_detail(format!(
            "getter decline target cross={:?} constructing={}",
            vm.cross_call_entry(getter).map(|target| unsafe { (*target).entry } as u8),
            constructing_in_lane(vm, getter)
        ));
        return Err(exit(ip));
    };
    let Some(this) = call_this_in_lane(vm, &target, base) else {
        return Err(exit(ip));
    };
    activate_kept_in_lane::<true>(cx, r, ip, &target, &[], this)
}

/// A `GetField` record's value where caches answer it (`LaneFrame::cached_read`): one read of a
/// register or `this`, or a nested site's chain of two. `None` leaves the read to the
/// general path.
///
/// Out of line: the generic reads (`lane_get_field`, `row_get_field_sink`, a returning
/// `GetField`) run it on their slower paths, so one copy serves them all instead of each
/// carrying its own probe (DeltaBlue, PGO build: ~40 instruction-cache lines of warm code).
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(never)]
pub(super) unsafe fn field_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The record's address names its private-name sites (`LaneFrame::private_slot`).
    let record = std::ptr::from_ref(i).addr();
    if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        let site = *code.field_sites.get(i.imm() as usize)?;
        let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
        let value = field_step_in_lane(cx, record + NESTED_STEPS[0], code, base, site.first.0, site.first.1)?;
        return match site.second {
            Some((atom, cache)) => field_step_in_lane(cx, record + NESTED_STEPS[1], code, value, atom, cache),
            None => Some(value),
        };
    }
    let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }?;
    field_step_in_lane(cx, record, code, base, i.imm(), i.c)
}

/// The object a field base names: a register, or `this` where the lane knows it.
///
/// # Safety
/// As for `field_value_in_lane`.
#[inline(always)]
pub(super) unsafe fn field_base_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: quench_runtime::bytecode::FieldBase,
) -> Option<Value> {
    if let Some(slot) = base.local_index() {
        return plain_local_operand(cx, slot);
    }
    match base.register_index() {
        Some(register) => Some(r.get(register)),
        None => unsafe { this_in_lane(cx, r) },
    }
}

/// The general write of a store past its generated row (`GENERAL_SITE`): a private name's store
/// (`lane_private_field_store`), else the public name's store made in place
/// (`row_set_field_in_place`). The row's own cached write already refused it.
#[inline(never)]
pub(super) fn row_set_field_miss_path(cx: *mut LaneContext, r: Registers, ip: Ip, object: Value, i: &LaneInstruction) -> Resumed {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    if vm.is_private_name(i.imm()) {
        if !lane_frame::LaneFrame::<ACC_NONE>::private_store(cx, ip.addr(), code, object, i.imm(), r.get(i.a)) {
            return Resumed::Exit(exit(ip));
        }
        // Adding a property can allocate.
        note_allocation(vm);
        return Resumed::At(r, following(ip), Carried::script(cx));
    }
    row_set_field_in_place(cx, r, ip, object, i)
}

/// `lane_set_field` of a public name past its caches: the general store made in place
/// (`in_place_in_lane`), in the frame's own run or beside frameless activations, strict as its
/// record is. A store that adds a property to receivers of many shapes (zod's schema
/// initializers in the playwright npm scenario: 51K of its 79K lane exits) otherwise left the
/// lane at every store.
#[cold]
#[inline(never)]
pub(super) fn row_set_field_in_place(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    object: Value,
    i: &LaneInstruction,
) -> Resumed {
    // The running function decides the store's strictness: a running leaf's is its
    // materialized activation's.
    materialize_running_leaf_in_lane(cx);
    if let Some(resumed) = AccessorCall::of(cx).store_setter(r, ip, i, object) {
        return resumed;
    }
    let Some(strict) = store_is_strict_in_lane(cx, ip) else {
        return Resumed::Exit(exit(ip));
    };
    let (atom, value, site) = (i.imm(), r.get(i.a), i.c);
    if let Some(resumed) = AccessorCall::of(cx).store_set_trap(r, ip, object, atom, value, strict) {
        return resumed;
    }
    if let Err(next) = unwound_in_place(cx, ip, |vm, code| vm.set_field_cached(code, object, atom, value, site, strict)) {
        return Resumed::Exit(next);
    }
    // Guest code the store ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// Whether the named field store at the record `ip` rejects a refused write with a
/// `TypeError`, as its opcode says; `None` for any other record.
pub(super) fn store_is_strict_in_lane(cx: *mut LaneContext, ip: Ip) -> Option<bool> {
    field_store_at(cx, ip).map(|store| store.strict)
}

/// The named field store at the record `ip`, as its opcode says; `None` for any other record.
pub(super) fn field_store_at(cx: *mut LaneContext, ip: Ip) -> Option<FieldStore> {
    let vm = unsafe { vm(cx) };
    let function = match vm.lane_stack.top_frame() {
        Some(activation) => activation.function(),
        None => vm.frames[unsafe { (*cx).frame() }].function,
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, record_pc(cx, ip) as usize) };
    let body = code.function(function as usize);
    match crate::compiled::decoded(body.widened(body.code[pc])) {
        // A store to `this` has no base register (`SetThisField`).
        Some(quench_runtime::staged::Operation::SetField { base, strict, .. }) => Some(FieldStore {
            receiver: if base.is_some() { StoreReceiver::Register } else { StoreReceiver::This },
            strict,
        }),
        _ => None,
    }
}

/// The brand of private name `atom` at the record `ip`, whose home follows the running
/// function and the environment its frame would capture from: the lane's frame, or a
/// frameless activation's function and closure environment (`Vm::site_private_brand`).
#[inline(always)]
pub(super) fn private_brand_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    code: &ResidualProgram,
    atom: quench_runtime::bytecode::Atom,
) -> Option<quench_runtime::tier_surface::PrivateBrand> {
    let (environment, function, frame) = private_site_in_lane(cx)?;
    unsafe { vm(cx) }.site_private_brand(code, ip.addr(), environment, function, frame, atom)
}

/// The running function's private-name scope: the environment its frame, or its activation,
/// captures from, the function, and whether it runs in a frame (`Vm::site_private_brand`).
#[inline(always)]
pub(super) fn private_site_in_lane(cx: *mut LaneContext) -> Option<(Value, u32, bool)> {
    let vm = unsafe { vm(cx) };
    Some(match vm.lane_stack.top_frame() {
        None => {
            // The running frame is the top one, whose home `Vm::private_name_home` finds.
            let frame = vm.frames.len().checked_sub(1)?;
            (vm.captured_parent_environment(vm.frame_caller(frame)), vm.frames[frame].function, true)
        }
        // SAFETY: the context outlives the lane run.
        Some(activation) => (unsafe { (*cx).registers }.env(), activation.script().map(|_| activation.function())?, false),
    })
}

/// One `GetField` step of `field_value_in_lane`: a private name's at its site, any other name's
/// through the field caches (`LaneFrame::cached_read`).
#[inline(always)]
pub(super) fn lane_private_field_slot(
    cx: *mut LaneContext,
    ip: Ip,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
) -> Option<usize> {
    lane_private_field_slot_at(cx, ip.addr(), code, object, atom)
}

/// `lane_private_field_slot` at site `site`: a record's address, or an address inside it for
/// a nested read's step (`lane_field_value`), which no other record's site can be. A site
/// whose name belongs to a class evaluated once (`PrivateClasses::unique_name`) answers from
/// the receiver's shape alone, before the running function's scope is found.
#[inline(always)]
pub(super) fn lane_private_field_slot_at(
    cx: *mut LaneContext,
    site: usize,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
) -> Option<usize> {
    {
        let vm = unsafe { vm(cx) };
        if vm.private_classes.unique_name(atom)
            && let Some(slot) = vm.private_site_homes.unique_field_slot(site, vm.object_data(object)?.shape())
        {
            return Some(slot);
        }
    }
    let scope = private_site_in_lane(cx)?;
    (unsafe { vm(cx) }).site_private_field_slot(code, site, scope, object, atom)
}

/// Makes the slot site `ip`'s entry recorded for an object of `shape` (`lane_private_field_slot`)
/// the hint of its `GetField` or `SetField` record (`lane_get_private_field`,
/// `lane_private_field_store`), where its private name `atom` belongs to a class evaluated
/// once: the entry recorded it under that class's home (`SiteHome::unique`), the site's for
/// good while the name stays unique, in a shape no edit in place changes, and a shape
/// compaction clears both. A record of a site that recorded none keeps its hint. A hit
/// compares the receiver's shape and reads the name's word (`PrivateClasses::unique_name`):
/// a second evaluation of the class, or eval code that sees the name, clears it, which
/// disables the hints of that name's sites alone.
#[inline(never)]
pub(super) fn hint_private_field(vm: &Vm, ip: Ip, atom: quench_runtime::bytecode::Atom, shape: u32, slot: usize) {
    if vm.private_classes.unique_name(atom)
        && vm.private_site_homes.unique_field_slot(ip.addr(), shape) == Some(slot)
        && let Ok(slot) = u16::try_from(slot)
        && slot != NO_CALL_LINK
    {
        set_field_hint(ip, quench_runtime::vm::FieldCache::own(shape, slot));
    }
}

/// One `GetField` step of `lane_field_value`: a private name's at its site, any other name's
/// through the field caches (`lane_cached_field`).
#[inline(always)]
pub(super) fn field_step_in_lane(
    cx: *mut LaneContext,
    site: usize,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
    cache: u16,
) -> Option<Value> {
    lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, object, atom, cache, site)
}

/// Raise at `ip` after its instruction failed with `error` before any effect, where a handler
/// of the running function or of a plain caller takes it in the lane
/// (`handler_edges::unwind_raised`). A frame run raises `error`; beside frameless activations,
/// whose frames `error`'s stack capture did not read, `again` makes the instruction's error a
/// second time beside them (`handler_edges::raise_again`, frame-switches.md #6).
#[inline(never)]
pub(super) fn raise_in_lane<T>(
    cx: *mut LaneContext,
    ip: Ip,
    error: JsError,
    again: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Ip {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        drop(error);
        return handler_edges::raise_again(cx, ip, again);
    }
    vm.lane_completion.raise(error);
    handler_edges::unwind_raised(cx, ip)
}

/// `DefinePropertyRecord` of the record at `ip`, defined as the general step defines it: a
/// class method or accessor, or an object literal accessor. An out-of-domain mode leaves the
/// instruction to the general path, a throw is raised at the record (`unwound_in_place`):
/// `Err` is the record the run goes on at. Out of line, as `row_index_write`:
/// the handler's frame then holds none of the descriptor.
#[inline(never)]
pub(super) fn define_property_record_in_place_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(), Ip> {
    let mode = quench_runtime::bytecode::PropertyDefinitionMode::from_word(i.imm()).ok_or(exit(ip))?;
    let (value, target, key) = (i.a, i.b, i.c);
    unwound_in_place(cx, ip, |vm, code| {
        let (value, target, key) = {
            let r = unsafe { (*cx).registers };
            (r.get(value), r.get(target), r.get(key))
        };
        let descriptor =
            quench_runtime::vm::object_descriptors::PropertyDescriptorRecord::for_definition(mode, value);
        vm.define_property_or_throw(code, target, key, descriptor)
    })
}

/// The edge a decided comparison takes: the jump is taken where it fails.
#[inline(always)]
pub(super) const fn holds_edge(holds: bool) -> quench_runtime::staged::Edge {
    if holds { quench_runtime::staged::Edge::Following } else { quench_runtime::staged::Edge::Target }
}

/// The tests the general loop decides past Number comparison without running guest code:
/// identity equality, an ordinary `instanceof`, BigInt order, and the truth of a Number
/// operator's result.
/// Out of line, so the comparison handlers keep their Number path's shape.
#[cold]
#[inline(never)]
pub(super) fn relation_other(operator: u32, left: Value, right: Value) -> Option<bool> {
    let vm = unsafe { vm(context_in_lane()) };
    // A nullish value is unequal to every Number under both loose and strict equality.
    // The Number/nullish pair cannot invoke conversion hooks or `document.all`; keep this
    // check in the shared cold fallback so it does not enlarge each hot comparison handler.
    if operator <= STRICT_INEQUALITY_OPERATOR
        && ((left.is_null() || left.is_undefined()) && right.as_number().is_some()
            || (right.is_null() || right.is_undefined()) && left.as_number().is_some())
    {
        return Some(matches!(operator, 1 | STRICT_INEQUALITY_OPERATOR));
    }
    // An ordering operator (`<`, `<=`, `>`, `>=`) is decided by its operand pair alone: the
    // membership, prototype and identity tests below never answer it.
    // `==` and `===` share `Equal` (`NumberRelation::from_operator`), which is no ordering: they take
    // the identity and string tests below.
    if quench_runtime::vm::operations::NumberRelation::from_operator(operator)
        .is_some_and(|relation| !matches!(relation, quench_runtime::vm::operations::NumberRelation::Equal | quench_runtime::vm::operations::NumberRelation::NotEqual))
    {
        return vm.string_relation(operator, left, right)
            .or_else(|| vm.bigint_relation(operator, left, right))
            .or_else(|| vm.primitive_comparison(operator, left, right))
            .or_else(|| vm.number_binary(operator, left, right)?.truthiness());
    }
    vm.instanceof_truthy(operator, left, right)
        .or_else(|| vm.plain_in(operator, left, right))
        .or_else(|| vm.identity_equality(operator, left, right))
        .or_else(|| vm.string_relation(operator, left, right))
        // A test of an arithmetic or bitwise result (`if (flags & MASK)`): the Number
        // operator's value, whose ToBoolean runs no guest code.
        .or_else(|| vm.bigint_relation(operator, left, right))
        // The remaining primitive pairs, whose conversions are pure: `undefined > 5`,
        // `flag == 1`, `"3" == 3`.
        .or_else(|| vm.primitive_comparison(operator, left, right))
        .or_else(|| vm.number_binary(operator, left, right)?.truthiness())
}

/// A taken jump. Backward jumps, the only ones that close a loop, are
/// safepoints: they leave the lane when a collection is due; forward jumps
/// dispatch directly. Every jump's direction is fixed when the view is
/// derived. Leaving hands the whole instruction to the general path, so
/// `commit`, the register write the instruction makes before its jump,
/// applies only when the lane takes the jump itself.
#[inline(always)]
pub(super) fn take_jump(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    backward: bool,
    view: Carried,
    commit: Option<(u16, Value)>,
) -> Resumed {
    let jump = unsafe { (*ip).jump() };
    // SAFETY: the view records the distance to a validated jump target.
    let target = unsafe { ip.byte_offset(jump) };
    // The backward edge spends fuel through the run's context the handler already holds
    // (`script_fuel_out`): a decrement of `LaneContext::fuel` and a branch, with no
    // thread-local load of the counter's address (`fuel_out`).
    if backward && script_fuel_out(cx) {
        return Resumed::Exit(match commit {
            None => safepoint_at(ip, target, view.acc),
            Some((register, value)) => safepoint_writing(r, ip, target, view.acc, register, value),
        });
    }
    if let Some((register, value)) = commit {
        r.set(register, value);
    }
    Resumed::At(r, target, view)
}

/// Count one backward jump of a crediting run (`LaneContext::backedges_left`); at the budget's
/// end, credit the running Wasm function with one interpreter entry for the loop work
/// (`ProgramStore::note_wasm_tier_backedges`) and refill it. A function entered once and
/// looping hot (JetStream's tsf and zlib) then promotes at its next entry, as one called often
/// does.
pub(super) fn count_backedge(cx: *mut LaneContext) {
    // SAFETY: called from `safepoint_at` with the run's context.
    let Some(left) = (unsafe { (*cx).backedges_left }) else {
        return;
    };
    if let Some(left) = left.checked_sub(1) {
        unsafe { (*cx).backedges_left = Some(left) };
        return;
    }
    let machine = unsafe { vm(cx) };
    let program = unsafe { (*cx).program };
    let function = machine
        .lane_stack
        .top_frame()
        .map_or_else(|| machine.frames[unsafe { (*cx).frame() }].function, |activation| activation.function());
    machine.programs.note_wasm_tier_backedges(program, function);
    unsafe { (*cx).backedges_left = Some(machine.programs.tiers().wasm.backedges_per_entry) };
}

/// `loop_native_in_lane`'s entry and run of `region` into `entry`: how it continues, or `None`
/// where its code is awaited or its entry record cannot be built (the site notes the decline).
#[inline(always)]
pub(super) fn loop_run_native_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    site: &LoopSite,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
) -> Option<LoopContinuation> {
    let wait = { let mut tier = site.tier.borrow_mut(); tier.awaits_code(unsafe { vm(cx) }, site).then(|| tier.take_wait()) };
    if let Some(wait) = wait {
        // The code is compiling: the back edges until the next look run the header untried
        // (`skips_entry`) rather than each paying the whole entry attempt (~200 Ir) to count
        // the wait down.
        site.skipped_entries.set(wait);
        site.count_decline(RegionDecline::Awaiting);
        return None;
    }
    let views = match loop_entry_checked_in_lane(cx, r, region, entry) {
        Ok(views) => views,
        Err(decline) => {
            site.count_decline(decline);
            site.note_decline();
            return None;
        }
    };
    // The tier's state is borrowed only here, so the dispatch after it holds no borrow.
    let continuation = loop_run_in_lane(cx, r, site, region, entry, views, site.header);
    count_continuation(cx, &continuation);
    if quench_runtime::fallback::on() { crate::compiled::fallback_sites::loop_left(cx, site, &continuation); }
    if matches!(continuation, LoopContinuation::Jump) { site.note_decline(); }
    Some(continuation)
}

/// Runs `site`'s region on `r` from its `header` record, reached at `record` (the backward
/// jump, or the header itself): the entry guards, then native code, which resumes the lane
/// where control leaves the region or leaves to the general path. Once the region has code,
/// the site holds its header record, so a loop entered from outside runs natively from its
/// first iteration.
#[inline(always)]
pub(super) fn loop_native_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    record: Ip,
    header: Ip,
    site: &LoopSite,
    region: &crate::compiled::LoopRegion,
) -> LoopAttempt {
    // SAFETY: the region carries its validated header and latch PCs, and every instruction
    // record remains contiguous in this lane view.
    let at = |pc: u32| unsafe { header.offset(pc as isize - site.header as isize) };
    if site.skips_entry() { return LoopAttempt::Declined; }
    // The record is filled where it lives, so its words are neither cleared nor copied.
    let mut entry = crate::compiled::LoopEntry::new();
    let Some(continuation) = loop_run_native_in_lane(cx, r, site, region, &mut entry) else { return LoopAttempt::Declined };
    loop_hold_header_in_lane(header, site);
    match continuation {
        LoopContinuation::Jump | LoopContinuation::Compiling => LoopAttempt::Declined,
        LoopContinuation::Resume(pc) => LoopAttempt::At(at(pc)),
        LoopContinuation::Exit(pc) if loop_body::left_for_tier_up(cx, site, pc) => LoopAttempt::At(at(site.header)),
        LoopContinuation::Exit(pc) => { count_arithmetic_leave(cx, site, pc, region.header..=region.backedge); LoopAttempt::Leave(exit(at(pc))) }
        LoopContinuation::Inlined { frame, pc } => { let frame = region.inlining.as_ref().expect("an inlining region").frames[frame];
            LoopAttempt::Leave(exit(inlined_activation_in_lane(cx, r, region, &entry, frame, at(frame.call), pc))) }
        LoopContinuation::Recovered(resumed) => loop_body::loop_attempt(cx, record, resumed),
        LoopContinuation::Safepoint => LoopAttempt::Leave(safepoint_at(record, header, Slot::unset())),
        // SAFETY: the call set the callee's activation and its resume record before the body
        // left (`body_activate_in_lane`), so the run leaves at a record of that view.
        LoopContinuation::Callee => LoopAttempt::Leave(exit(unsafe { (*cx).callee_resume })),
        LoopContinuation::Completed(pc, value) =>
            LoopAttempt::Resumed(return_value_general_in_lane(cx, at(pc), value)),
        LoopContinuation::Threw(pc) => LoopAttempt::Leave(raised_exit_in_lane(cx, at(pc))),
    }
}

/// Counts a leave at `pc` (`LoopTier::left_at`) where it is a Number operator, whose int32
/// speculation a compilation can drop, an element read, whose observed type it can drop, an
/// element store, whose append helper it can compile, a field store, whose VM store it can call
/// (`Lowering::store_leaves`), or a capture read, whose root-held binding
/// it can read by helper; a leave at any other guard (a shape, say) widens nothing.
#[cold]
pub(super) fn count_arithmetic_leave(cx: *mut LaneContext, site: &LoopSite, pc: u32, region: std::ops::RangeInclusive<u32>) {
    if !region.contains(&pc) { return; }
    // SAFETY: the lane's VM is live, and nothing borrows it across the read.
    let machine = unsafe { vm(cx) };
    let counted = machine.programs.get(machine.active_program).as_deref().and_then(|program| {
        let body = program.function(site.function() as usize);
        body.code.get(pc as usize).and_then(|packed| {
            let instruction = body.widened(*packed);
            let op = instruction.op();
            crate::compiled::base_region::leave(site.function(), pc, op);
            let stores = quench_runtime::staged::stores_field(op) && matches!(op.staged(), Some(quench_runtime::bytecode::Decoder::Property)); // what decodes to `SetField` (`decode_property`), with no decode
            (quench_runtime::staged::computes_numeric_binary(op) || quench_runtime::staged::reads_element(op) || op.stores_element()
                || stores || quench_runtime::staged::miss(op) == quench_runtime::staged::Miss::WalkScopes)
                .then(|| (quench_runtime::staged::reads_element(op), stores))
        })
    });
    match counted {
        Some((_, true)) => site.tier.borrow_mut().left_store(site.tiers, pc as usize),
        Some((read, false)) => site.tier.borrow_mut().left_at(site.tiers, pc as usize, read),
        None => {}
    }
}

/// Gives `site` its `header` record (`LoopSite::hookable`) once its region has code: the
/// record runs `loop_header_miss`, and the site keeps the record's own handler.
pub(super) fn loop_hold_header_in_lane(header: Ip, site: &LoopSite) {
    // A retired site (`RETIRED_HEADER`) never holds its header again: its jump record is a plain
    // jump and the header's `call_link` a field hint, so `header_site_in_lane` would read garbage.
    if !site.hookable || !site.header_handler.get().is_null() || !site.tier.borrow().is_published() {
        return;
    }
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view.
    unsafe {
        site.header_handler.set((*header).handler);
        (*header.cast_mut()).handler = loop_header_miss as Handler as *const ();
    }
}

/// The site whose header record is at `ip` (`LoopSite::hookable`): the header's `call_link`
/// is the distance, in records, to the site's backward jump, whose operand is the site
/// (`install_loop_sites`). A held header's jump still holds it: retiring the site gives the
/// header its own handler back first (`loop_retire_in_lane`).
///
/// # Safety
/// `ip` is a held header record of a live view.
#[inline(always)]
pub(super) unsafe fn header_site_in_lane(ip: Ip) -> *const LoopSite {
    // SAFETY: the jump lies `call_link` records past its header in the same view.
    unsafe { (*ip.add(usize::from((*ip).call_link.get()))).loop_site() }
}

/// Counts an entry of `function`'s lane activation toward its lane body (`own`), or, `now`,
/// specializes it at once, from the targets and callees the program's views hold now. Whether the body
/// is still warming or specialized (`LaneView::specialize_lane_body`).
pub(super) fn specialize_in_lane(cx: *mut LaneContext, own: &LaneView, function: u32, now: bool, parameters: &[Value], looped: Option<(u64, u64)>) -> bool {
    if !now && own.counted_between_attempts(false) { return true; }
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The entry activation's array parameters, sampled: a lane body's loops read them typed (`observed_reads`).
    // Where the body comes from is the specializer's decision (`admission::route_for_lane`): an ahead image (the
    // ladder's first body, no warm-up; one that left too often `respecializes` on the JIT's route), the stencil
    // tier at once, a wait for callees still compiling, or the lane body tier.
    let ahead_image = !own.respecializes() && code.kind != quench_runtime::bytecode::ProgramKind::Wasm && vm.programs.ahead_images(unsafe { (*cx).program }, code).is_some();
    if crate::compiled::admission::route_for_lane(crate::compiled::admission::LaneRouteFacts { ahead_image, callees_compiling: false })
        == crate::compiled::admission::LaneRoute::Ahead
    {
        // An image body takes a claimed prologue, whichever entry reached it first: its entry reads the function there.
        if let Some(body) = code.function_get(function as usize) && unsafe { (*own.prologue.get()).handler }.is_null() { own.claim_for_tier(tier_prologue_row(), function, body.params, !body.is_arrow); }
        let published = own.entry_published();
        let installed = own.install_ahead_lane_body(body_entry_row(), || crate::compiled::admission::ahead_unit(function).and_then(|unit| vm.install_ahead_body(unit, code, function, None, &[])));
        if !published && own.entry_published() {
            unsafe { super::vm(cx) }.unprove_view_entries(own);
        }
        return installed;
    }
    // The JIT's routes link only into a binary whose runtimes it serves (`jit_routes`): without
    // them, a function with no ahead image declines its lane body, as under `--no-jit`.
    let jit = crate::jit_routes::jit().map(|routes| routes.lane);
    let specialize = |work: crate::compiled::LaneBodyWork| jit.map_or(crate::compiled::LaneBodyOutcome::Declined, |jit| jit(cx, own, function, parameters, looped, work));
    let published = own.entry_published();
    let warming = if now {
        own.specialize_lane_body_now(body_entry_row(), looped.is_some(), specialize);
        own.body_view_in_lane().is_some()
    } else {
        own.specialize_lane_body(body_entry_row(), false, specialize)
    };
    // Every cell body is published here (`tier_machine::publish` from `specialized`): the
    // proved calls of the view, which skip the cell body, lose their proof.
    if !published && own.entry_published() {
        unsafe { super::vm(cx) }.unprove_view_entries(own);
    }
    warming
}

/// Works toward `function`'s lane body on the JIT's routes: the stencil tier, a wait for callees
/// still compiling, or the lane body tier (`admission::route_for_lane`).
pub(crate) fn jit_lane_body(cx: *mut LaneContext, own: &LaneView, function: u32, parameters: &[Value], looped: Option<(u64, u64)>, work: crate::compiled::LaneBodyWork<'_>) -> crate::compiled::LaneBodyOutcome {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let diagnostics = crate::compiled::admission::diagnosing(vm);
    let arrays = loop_tier::sampled_arrays(vm, parameters.iter().copied());
    let work = match work {
        // A looping caller's trips pay for the body as its own iterations do (`looped`).
        crate::compiled::LaneBodyWork::Start { generic, shrinks, observed, looped: seen, called, caller, .. } =>
            crate::compiled::LaneBodyWork::Start { generic, shrinks, observed, looped: looped.or(seen), called, caller, arrays: &arrays },
        poll => poll,
    };
    let direct = |vm: &Vm| code.function_get(function as usize).map(|body| direct_callees_in_lane(vm, own, function, code, body)).unwrap_or_default();
    // A start waits for callees still compiling (`body_tier_in_lane::callees_compiling`), calling their bodies by their entries.
    let starting = matches!(work, crate::compiled::LaneBodyWork::Start { .. });
    let callees_compiling = starting && body_tier_in_lane::callees_compiling(vm, own, code.function_get(function as usize));
    match crate::compiled::admission::route_for_lane(crate::compiled::admission::LaneRouteFacts { ahead_image: false, callees_compiling }) {
        // `--b-jit` stitches its body at once: it calls the callees its sites recorded by their entries and waits for none.
        crate::compiled::admission::LaneRoute::Base => {
            let crate::compiled::LaneBodyWork::Start { generic, .. } = &work else { unreachable!("a stencil body never compiles in the background") };
            let unit = match vm.base_unit_or_outcome(code, function, parameters, own) { Ok(unit) => unit, Err(outcome) => return outcome };
            return vm.base_body_work(unit, code, function, generic, parameters, own, &direct);
        }
        crate::compiled::admission::LaneRoute::Deferred => return crate::compiled::LaneBodyOutcome::Deferred,
        crate::compiled::admission::LaneRoute::Ahead => unreachable!("an ahead image's program returned above"),
        crate::compiled::admission::LaneRoute::Lane => {}
    }
    let outcome = vm.body_work_in_lane(code, function, &|function, pc| unit_targets_in_lane(cx, function, pc), &|function, header| unit_trips_in_lane(cx, function, header), &|callee| callee_captures_in_lane(cx, callee), &direct, work, None);
    if diagnostics && let crate::compiled::LaneBodyOutcome::Published(method) = &outcome {
        if let Some(method) = method.lease() { own.tier().body_stats.compiled(function, &method.inlined); }
    }
    outcome
}

/// Gives a loop site that will not enter native code again its plain backward jump back,
/// so each iteration takes the jump without the site's checks.
pub(super) fn loop_retire_in_lane(cx: *mut LaneContext, ip: Ip, site: &LoopSite) {
    // A hot loop no region runs makes its function worth a body past the entry warm-up: its
    // calls have run, so their sites have linked, and its next entry runs the loop natively.
    // A body still warming after the loop's trips asked for it is refused under the body's epoch
    // (`LoopSite::refuse_warming`): its back edges are plain jumps until the epoch moves or the
    // wait runs out, so a function whose entries do not advance costs no per-edge checks.
    // SAFETY: the program's views outlive the lane run.
    if crate::compiled::admission::lane_bodies_enrolled(unsafe { vm(cx) })
        && let Some(own) = unsafe { &*(*cx).views }.get(site.function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)
    {
        specialize_in_lane(cx, own, site.function, true, &[], Some((site.span(), site.body_iterations())));
        // A body the model priced with the loop's work and found unpaid gains nothing from the loop asking
        // again: the loop retires to its plain jump, which costs no countdown per back edge (TOD-2062).
        if own.warming() && !own.body_unpaid() { return site.refuse_warming(own, site.tiers.loops.warmup_taken); }
    }
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view, and
    // the jump reads its distance from the operand, which the site kept (`LoopSite::jump`).
    // A header record the site holds gets its own handler back.
    let header = unsafe { ip.byte_offset(site.jump) };
    loop_release_header_in_lane(header, site);
    // A run of this site still on the stack (a native attempt that retired it) must not hold
    // the header again on its way out (`loop_hold_header_in_lane`).
    site.header_handler.set(RETIRED_HEADER);
    // A retired loop enters no native code, so the loops inside it take their own regions (`LoopSite::inside_admitted_loop`).
    site.entering.set(false);
    // No site holds the header from now on, so its record, which linked this site alone, takes
    // field hints again (`holds_loop_site`).
    if site.hookable { unsafe { (*header).call_link.set(NO_CALL_LINK) }; }
    unsafe { (*ip.cast_mut()).operand = LaneOperand { jump: site.jump }; }
    requicken(ip, jump_row::<JUMP>(true));
    // The site kept the update before the jump from fusing with it (`install_loop_sites`
    // runs before `fuse_rows`): the plain jump takes the loader's pair fusion now, with the
    // `FUSED_TARGET` record where the header's target run holds (`update_fusion`'s `target`).
    if site.backedge > site.header {
        // A header comparison that ran while the site held its loop took its formed instance (`row_jump`),
        // which no target run names: it goes back to its plain row (and its record's `mode` to the zero of an
        // unfused record), as the loader's `fuse_rows` saw it, so the back edge fuses the test again.
        // SAFETY: the header is a record of the running view, on this thread.
        if let Some(plain) = lane_frame::unformed_compare_jump(unsafe { (*header).handler }) {
            unsafe { (*header.cast_mut()).mode = 0 };
            requicken(header, plain);
        }
        // SAFETY: the record before the backedge is the loop body's, in the same view as the jump.
        let update = unsafe { ip.sub(1) };
        if let Some((fused, mode)) = update_fusion(unsafe { &*update }, unsafe { &*ip }, Some(header)) {
            // SAFETY: the record is the running view's, on this thread; its handler is requickened next.
            unsafe { (*update.cast_mut()).mode = mode };
            requicken(update, int_steps::retired_step(cx, site, update, unsafe { &*header }, fused));
        }
    }
}

pub(super) fn loop_release_header_in_lane(header: Ip, site: &LoopSite) {
    let held = site.header_handler.replace(std::ptr::null());
    if !held.is_null() && held != RETIRED_HEADER {
        // SAFETY: the validated site owns this record on the running view's thread.
        unsafe { (*header.cast_mut()).handler = held; }
    }
}

/// Counts what a loop run came to: in the VM's tier events, and under `QUENCH_TRACE=js-jit` in the report's.
pub(super) fn count_continuation(cx: *mut LaneContext, continuation: &LoopContinuation) {
    use crate::compiled::base_region::{count, Event as Region};
    let (region, tier) = match continuation {
        LoopContinuation::Jump | LoopContinuation::Compiling => (Region::Declined, None),
        LoopContinuation::Exit(_) | LoopContinuation::Inlined { .. } | LoopContinuation::Recovered(_) | LoopContinuation::Safepoint | LoopContinuation::Callee | LoopContinuation::Threw(_) => (Region::Left, Some(Event::OsrLeft)),
        LoopContinuation::Resume(_) | LoopContinuation::Completed(..) => (Region::Entered, Some(Event::OsrEntered)),
    };
    count(region);
    if let Some(event) = tier { unsafe { vm(cx) }.programs.tier_events().record(event); }
}

/// Compiles `site`'s region if it has no current code, for the element `views` its entry
/// proved, then runs it on `r` with `entry`. A representation an outcome shows wrong is
/// widened, and the code dropped for the next compilation.
#[inline(never)]
pub(super) fn loop_run_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    site: &LoopSite,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
    views: crate::compiled::ElementViews,
    from: u32,
) -> LoopContinuation {
    use crate::compiled::{LaneExit, RegionRun};
    let mut tier = site.tier.borrow_mut();
    if !tier.select(site.tiers, views) {
        return site.declined(RegionDecline::Views);
    }
    if !tier.is_published() {
        // SAFETY: the lane's VM is live, and nothing borrows it across the read.
        if !tier.may_run(site.tiers, unsafe { vm(cx) }) {
            return site.declined(RegionDecline::NoCompile);
        }
        let number_arrays = loop_tier::number_arrays(unsafe { vm(cx) }, r, region);
        let int_entries = loop_tier::int_entries(r, region);
        let typed_captures = loop_tier::typed_capture_slots(cx);
        let int_captures = loop_tier::int_capture_slots(cx);
        loop_compile_in_lane(cx, r, site, region, &mut tier, &number_arrays, &int_entries, &typed_captures, &int_captures);
        if !tier.is_published() {
            site.count_decline(RegionDecline::Compiling);
            return LoopContinuation::Compiling;
        }
    }
    // A b-jit region due to move to u-jit starts the u-jit compile and runs on meanwhile
    // (`LoopTier::promote`); one not yet due runs until the u-jit region would be ahead.
    if tier.promotion_due() && tier.may_run(site.tiers, unsafe { vm(cx) }) {
        let number_arrays = loop_tier::number_arrays(unsafe { vm(cx) }, r, region);
        let int_entries = loop_tier::int_entries(r, region);
        let typed_captures = loop_tier::typed_capture_slots(cx);
        let int_captures = loop_tier::int_capture_slots(cx);
        loop_compile_in_lane(cx, r, site, region, &mut tier, &number_arrays, &int_entries, &typed_captures, &int_captures);
        if !tier.is_published() { return LoopContinuation::Compiling; }
    }
    if let Some(budget) = tier.backedge_budget(site.tiers) { entry.set_backedge_budget(budget); }
    // The code alone is held for the run (its publication is the tier's): one count, not two.
    let code = tier.code.as_ref().and_then(|publication| publication.lease()).expect("compiled above");
    drop(tier);
    // The native field probe may access the VM only through its synchronous, cache-only
    // callback. Pass raw pointers so no Rust borrow of the VM overlaps that callback.
    let (machine, program, interrupt) = unsafe {
        let context = &*cx;
        (
            context.vm,
            context.program,
            std::ptr::addr_of!((*context.vm).execution_interrupt),
        )
    };
    // SAFETY: `r` and `machine` are the running lane activation's storage and VM; the callback executes no guest code.
    let run = match unsafe { region_run_in_lane(cx, r, entry, (&*code, region), (site.function, from), (machine, program, interrupt)) } {
        Ok(run) => run,
        // The lane admitted no run, so the region's code entered nothing and the interpreter takes the jump.
        Err(decline) => return site.declined(decline),
    };
    if site.diagnostics {
        loop_diagnose_in_lane(cx, r, site, region, run, program);
    }
    let RegionRun { exit, nested } = run;
    if exit.entered_nothing() {
        // An entry guard missed: registers are untouched, so the backward jump runs in the interpreter.
        site.tier.borrow_mut().missed_entry(site.tiers);
        return crate::compiled::fallback_sites::entry_refused(cx, site, if matches!(exit, LaneExit::Stub) { RegionDecline::Stub } else { RegionDecline::EntryContract });
    }
    if matches!(exit, LaneExit::Terminated) { return LoopContinuation::Safepoint; }
    crate::compiled::admission::retire_after_native(unsafe { vm(cx) }, || int_steps::native_handed_doubles(cx, site, exit, |register| r.get(register).as_int().is_some()));
    unsafe { vm(cx) }.refresh_body_bases(); // the region reloaded only its own payload's bases, not a body frame's below it
    // A run pays for its entry with the iterations it completed, wherever it leaves.
    site.note_run(entry.backedge_count(), region.holds_exit(exit) || matches!(exit, LaneExit::Inlined { .. }));
    if nested {
        return region_left_in_lane(cx, r, site.function, exit);
    }
    match exit {
        LaneExit::At(pc) if region.holds_exit(exit) => LoopContinuation::Exit(pc as u32),
        LaneExit::At(pc) => LoopContinuation::Resume(pc as u32),
        LaneExit::Inlined { frame, pc } => inlined_continuation_in_lane(cx, r, site, (region, &code), (frame, pc)),
        // The call made the callee's activation, whose own records the lane runs next.
        LaneExit::Callee => LoopContinuation::Callee,
        LaneExit::Returned { pc, value } => LoopContinuation::Completed(pc as u32, value),
        LaneExit::Threw { pc } => LoopContinuation::Threw(pc as u32),
        LaneExit::Entry | LaneExit::Stub | LaneExit::Terminated => unreachable!("handled before the run paid"),
    }
}

/// The diagnostics of a loop site's native attempt that ended in `run`, under its `site.diagnostics`.
pub(super) fn loop_diagnose_in_lane(cx: *mut LaneContext, r: Registers, site: &LoopSite, region: &crate::compiled::LoopRegion, run: crate::compiled::RegionRun, program: ProgramId) {
    use crate::compiled::LaneExit;
    let bump = |count: &std::cell::Cell<u64>| count.set(count.get().saturating_add(1));
    bump(&site.native_entry_attempts);
    if run.nested {
        bump(&site.native_region_runs);
        return;
    }
    match run.exit {
        LaneExit::Entry | LaneExit::Stub => {
            if quench_runtime::trace::Channel::OsrReject.on() {
                eprintln!("QUENCH_JS_JIT_OSR_ENTRY_MISS function={} header={}", site.function, site.header);
            }
            bump(&site.entry_guard_misses);
        }
        LaneExit::Terminated => {}
        LaneExit::Inlined { .. } => {
            bump(&site.native_region_runs);
            bump(&site.native_exits);
        }
        // A guard's exit inside the loop; the poll's leave for a published body is a handover, not one.
        LaneExit::At(pc) if region.holds_exit(run.exit) && !loop_body::left_for_tier_up(cx, site, pc as u32) => {
            bump(&site.native_region_runs);
            bump(&site.native_exits);
            loop_trace_exit_in_lane(cx, r, site, region, program, pc as u32);
        }
        _ => bump(&site.native_region_runs),
    }
}

/// Attributes a guard exit at `pc` to its recovery PC before the lane resumes, under the opt-in
/// OSR trace; ordinary execution keeps no extra table or per-site allocation.
pub(super) fn loop_trace_exit_in_lane(cx: *mut LaneContext, r: Registers, site: &LoopSite, region: &crate::compiled::LoopRegion, program: ProgramId, pc: u32) {
    if quench_runtime::trace::Channel::OsrReject.on() {
        eprintln!(
            "QUENCH_JS_JIT_OSR_EXIT program={} function={} header={} pc={pc}",
            program.raw(), site.function, site.header,
        );
        // Read recovered operands only for a non-inlined activation. These
        // getters neither coerce a guest value nor allocate guest storage.
        // Raw state distinguishes index/append/capacity guards without
        // changing native exits or inventing a second guard policy.
        if region.inlining.is_none() {
            // SAFETY: the lane's program and VM outlive this synchronous trace;
            // native execution has returned and restored the canonical registers.
            let body = unsafe { &*(*cx).code }.function_get(site.function as usize);
            if let Some((body, packed)) = body.and_then(|body| body.code.get(pc as usize).map(|packed| (body, *packed))) {
                let instruction = body.widened(packed);
                if quench_runtime::staged::writes_element(instruction.op()) {
                    let key = r.get(instruction.register_c());
                    let base = r.get(instruction.register_b());
                    if let Some(array) = unsafe { vm(cx) }.heap_reads().record_array(base) {
                        eprintln!(
                            "QUENCH_JS_JIT_OSR_SET_INDEX_EXIT program={} function={} header={} pc={pc} key_bits={:016x} key_number={:?} length={} capacity={}",
                            program.raw(), site.function, site.header,
                            key.to_bits(), key.as_number(),
                            array.elements().len(), array.elements().capacity(),
                        );
                    }
                }
            }
        }
    }
}

/// Runs `code`, the region of the loop of `function` over the activation with registers `r`, from
/// its header or the record `from`, where the lane admits the run (`region_admits_in_lane`); a run it
/// does not admit enters nothing, and the `RegionDecline` names why.
///
/// # Safety
/// As `CompiledOsrRegion::run`: `machine` and `interrupt` belong to the running lane.
pub(super) unsafe fn region_run_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    entry: &mut crate::compiled::LoopEntry,
    (code, region): (&crate::compiled::CompiledOsrRegion, &crate::compiled::LoopRegion),
    (function, from): (u32, u32),
    (machine, program, interrupt): (*mut Vm, ProgramId, *const quench_runtime::execution_interrupt::ExecutionInterrupt),
) -> Result<crate::compiled::RegionRun, RegionDecline> {
    // A job's frameless lane (a generator step) has no frame for the region's activation.
    if code.needs_frame() && !unsafe { &*cx }.has_frame() {
        return Err(RegionDecline::Frame);
    }
    if !region_admits_in_lane(cx, code) {
        // A run the lane lacks room for grows it and leaves at the loop's header, so the next entry admits.
        let grown = frame_holds_window_in_lane(cx, &code.calls) && room_prepare_in_lane(cx, code.calls.recovery_space());
        return Ok(crate::compiled::RegionRun::own(if grown { crate::compiled::LaneExit::At(region.header as usize) } else { crate::compiled::LaneExit::Entry }));
    }
    // SAFETY: the caller's contract.
    Ok(unsafe { code.run(r.0, entry, region, machine, program, function, (*cx).frame(), interrupt, (cx as usize, direct_calls_in_lane(cx)), from) })
}

/// Whether a run of `code`, the region of the loop of `function` the lane is in, may start: the lane
/// has the room to make the activations of the calls it inlines should it leave inside one
/// (`room_admits_in_lane`), and the activation keeps its locals apart from the window past its
/// registers, which the registers of those callees occupy (`CompiledOsrRegion::run_lane`).
#[inline(always)]
pub(super) fn region_admits_in_lane(cx: *mut LaneContext, code: &crate::compiled::CompiledOsrRegion) -> bool {
    let room = code.calls.recovery_space();
    room.activations == 0 || (room_admits_in_lane(cx, room, NO_PENDING_ACTIVATIONS) && frame_holds_window_in_lane(cx, &code.calls))
}

/// Whether the running activation's register storage holds the windows of the callees `calls` inlines,
/// which start past the function's environment window (`base_region::function::inline_base`): a lane
/// activation's value stack has the room `room_admits_in_lane` proved (`CallCode::recovery_space` covers
/// the reach); a VM frame's register file, sized to its own words, needs that much capacity past its
/// registers (`INLINED_WINDOW_ROOM`, which an environment window reserves).
#[inline(always)]
pub(super) fn frame_holds_window_in_lane(cx: *mut LaneContext, calls: &crate::compiled::CallCode) -> bool {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        return true;
    }
    frame_reaches_in_lane(cx, calls.caller_registers(), calls.window())
}

/// Whether the running VM frame's register file holds `reach` words past its `registers`, where code
/// writes past them (`base_region::function::inline_base`); a lane activation's storage, at the top of
/// the value stack, is proved by its room instead (`room_admits_in_lane`).
#[inline(always)]
pub(super) fn frame_reaches_in_lane(cx: *mut LaneContext, registers: u16, reach: usize) -> bool {
    let vm = unsafe { vm(cx) };
    reach == 0 || vm.lane_stack.has_frames()
        || vm.frames.get(unsafe { (*cx).frame() }).is_some_and(|frame| frame.registers.capacity() >= usize::from(registers) + reach)
}

/// Whether the running activation keeps its locals in the window past its registers, as
/// `body_locals` finds them for an OSR region (`LocalsOf::Osr`): an environment activation does, and so does a VM frame whose
/// own storage does not cover the function.
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_locals_in_window_in_lane(context: usize) -> bool {
    let cx = context as *mut LaneContext;
    let vm = unsafe { vm(cx) };
    if let Some(activation) = vm.lane_stack.top_frame() {
        return activation.script().is_some_and(|script| script.environment);
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // A function none of whose records addresses a local (`CallPlan::addresses_locals`) keeps
    // none anywhere: its frame holds none, and neither its records nor its bodies read any.
    vm.frames.get(unsafe { (*cx).frame() }).is_some_and(|frame| code.function_get(frame.function as usize)
        .is_some_and(|body| frame.locals.len() < usize::from(body.locals) && body.call_plan(code).addresses_locals))
}
