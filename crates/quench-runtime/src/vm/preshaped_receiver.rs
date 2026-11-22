//! Receivers made in their constant layout: a base class's construction allocates its receiver
//! already holding the leading fields its constructor defines first thing with primitive
//! constants (`bytecode::ConstantField`, plain JavaScript or TypeScript), or, for a TypeScript
//! class, the leading fields it declares without initializers (`DeclaredLayout`), so those defines
//! find their slots and every instance has the class's layout from birth.
//!
//! Invariant: a receiver of `constructor` made from a template of shape `start` (empty storage)
//! is given the shape that adding the layout's names in order to `start` reaches, each value the
//! constant its define stores, and nothing else. This is exactly the object the constructor's
//! first `DefineField`s make (`compile::class::emit_instance_fields`): a base class's
//! [[Construct]] runs InitializeInstanceElements right after OrdinaryCreateFromConstructor,
//! before the body and the parameter bindings (whose destructuring and defaults run after the
//! fields); a field whose initializer is absent or a primitive constant evaluates no code and
//! defines its value by CreateDataPropertyOrThrow, which on a fresh extensible ordinary object
//! cannot fail and runs no code (no setter, Proxy trap or prototype is consulted by a define);
//! and the layout stops at the first field that is not of that kind (a computed, private,
//! numeric or `__proto__` key, an `accessor`, or any other initializer). Until that field, no
//! code runs and nothing but the constructor's frame holds the receiver, so no `in`,
//! `Object.keys`, `hasOwnProperty`, getter or Proxy can see the properties exist earlier than JS
//! defines them; a later initializer that reaches the receiver, or throws with it saved, sees
//! exactly the leading fields, as the defines leave it. The defines themselves then find an own
//! writable, enumerable, configurable data property and write their value over it (the plain
//! overwrite of `proved_field_define_hit`), keeping the key order. Private methods (marked
//! before the fields) and private fields (after) touch no public key.
//!
//! The skipped run: where a base class constructor's run of constant fields is two or more, the
//! compiler emits `ConstantFieldsHeld` of the receiver and a `JumpTrue` past the run's defines
//! (and the initializers' constant loads) before it (`compile::class::emit_instance_fields`); the
//! jump is taken where the receiver already holds the run (`Vm::constant_fields_held`). That test
//! is exact on its own terms: the receiver's exact shape must be the one the run's first define
//! (its own cache site, which no other instruction writes) found already holding its name
//! (`field_define_overwrite`). At that define a fresh receiver holds the name only where it was
//! made in the constant layout, as nothing before it adds a public key (the template is empty,
//! private brands are no keys, the run's names are distinct), so that shape holds every name of
//! the run in order with default attributes; a receiver of that shape at the run's start was made
//! so too, and holds the constants, which the defines would write over themselves. Any receiver
//! that was not (a construct plan's empty template, an inline allocation of another tier, a run
//! longer than this module gives) fails the test and runs the defines. Not taking the jump is
//! always exact, so a tier may run the test or not.
//!
//! The layout depends on the constructor's body alone: whichever `new.target` a construction
//! passes (a subclass's `super()`, `Reflect.construct`, a Proxy whose `prototype` trap ran
//! before the allocation), its receiver is fresh, of `new.target`'s prototype, and runs this
//! body's defines next. Entries are keyed by constructor and template shape, so each evaluation
//! of a class (a new constructor) and each prototype's template shape derives its own. Any other
//! path that allocates an empty receiver for the same class stays correct: its defines add the
//! fields one by one, reaching the same shape, as before.
//!
//! Receivers made with their learned storage (slack tracking): any other constructor's receiver,
//! plain JavaScript or a class whose fields have other initializers, starts with no properties
//! and gains them one add at a time, each along the transition its site caches. What it reaches
//! is learned per `new.target` (`LearnedReceiver`) from the receivers already made: the storage
//! length of the last one when the next is allocated, the most of a few per young cycle. A
//! receiver of a constructor that reached more values than the inline slots hold (and than its
//! constant layout gives it) is allocated over an empty young run reserved for that many
//! (`Heap::reserve_receiver_run`); a constant layout's values are appended into it first, then
//! its adds append in place (`Heap::record_property_append_reserved`) instead of moving its
//! values from the inline slots to the arena and then from class to class as it grows.
//!
//! Invariant: only storage is learned, never a shape. The receiver keeps the template's shape and
//! prototype (or the constant layout's shape over them), and every add still runs the full [[Set]]
//! or define, through its cache or not, and walks the same transitions, so whatever the chain
//! holds (setters, read-only names, a Proxy, a frozen or non-extensible prototype), whatever the
//! body does (throws midway, returns another object, deletes or redefines what it added) and
//! whichever `new.target` a `super()` passes, every object is exactly the one the unlearned
//! construction makes. A wrong count costs only memory (unused reserved words) or the moves it
//! would have saved.
use super::*;
use crate::bytecode::FieldConstant;
use crate::heap::InlineObjectHeader;
use crate::value_vec::INLINE_PROPERTY_COUNT;

/// Entries in the direct-mapped `Vm::preshaped_receivers` memo; a power of two.
pub(super) const PRESHAPED_RECEIVER_ENTRIES: usize = 64;
/// The most fields a layout gives a receiver at its allocation: a longer constant layout gives
/// its first ones (still a leading run), a longer declared layout none; the defines add the rest.
/// The compiler's skip over the run covers as many (`bytecode::PRESHAPED_FIELDS`).
pub(super) const PRESHAPED_FIELDS: usize = crate::bytecode::PRESHAPED_FIELDS;

/// What a construction by `constructor` from a template of shape `start` allocates: the shape its
/// layout's `len` names (`atoms`) reach from `start`, each in turn reaching `shapes`, holding
/// `values`, or nothing more (`len` 0). Cleared by every collection, which may free `constructor`
/// or a value's string and renumber shapes, and with the heap (`forget_learned_receivers`).
#[derive(Clone, Copy)]
pub(super) struct PreshapedReceiver {
    constructor: Value,
    start: u32,
    shape: u32,
    len: u8,
    atoms: [Atom; PRESHAPED_FIELDS],
    shapes: [u32; PRESHAPED_FIELDS],
    values: [Value; PRESHAPED_FIELDS],
}

impl PreshapedReceiver {
    /// An entry no construction matches: no constructor is `undefined`.
    pub(super) const EMPTY: Self = Self {
        constructor: Value::UNDEFINED,
        start: u32::MAX,
        shape: u32::MAX,
        len: 0,
        atoms: [0; PRESHAPED_FIELDS],
        shapes: [u32::MAX; PRESHAPED_FIELDS],
        values: [Value::UNDEFINED; PRESHAPED_FIELDS],
    };

    #[inline(always)]
    fn index(constructor: Value, start: u32) -> usize {
        let handle = constructor.heap_index().unwrap_or_default() as usize;
        (handle ^ start as usize) & (PRESHAPED_RECEIVER_ENTRIES - 1)
    }

    /// An entry that gives the receiver nothing.
    fn none(constructor: Value, start: u32) -> Self {
        Self { constructor, start, shape: start, ..Self::EMPTY }
    }
}

/// Entries in the direct-mapped `Vm::learned_receivers` table; a power of two.
pub(super) const LEARNED_RECEIVER_ENTRIES: usize = 64;
/// The most values a learned receiver's run is reserved for; a constructor that adds more grows
/// past it as before.
const LEARNED_SLOTS_MAX: usize = 32;
/// The receivers each young cycle samples per `new.target` before its count holds for the rest
/// of the cycle.
const LEARNED_SAMPLES: u8 = 4;

/// What the receivers of constructions with `new_target` reached (the module's slack tracking):
/// the storage length `slots` they reach, sampled `samples` times this young cycle from the
/// previous receiver, `last`, when the next is allocated; and the receiver template of layout
/// `template` (inline storage, no properties, no extras) with `header`, its prototype and shape
/// over arena storage, which a reserved receiver takes. Kept across collections, which only
/// forget `last` and restart the sampling (`forget_learned_receiver_samples`): `new_target` and
/// the layouts only key a hint, and any count is sound.
#[derive(Clone, Copy)]
pub(super) struct LearnedReceiver {
    new_target: Value,
    last: Value,
    template: u32,
    header: InlineObjectHeader,
    slots: u8,
    samples: u8,
}

impl LearnedReceiver {
    /// An entry no construction matches: no `new.target` is `undefined`.
    pub(super) const EMPTY: Self = Self {
        new_target: Value::UNDEFINED,
        last: Value::UNDEFINED,
        template: u32::MAX,
        header: InlineObjectHeader::EMPTY,
        slots: 0,
        samples: 0,
    };

    #[inline(always)]
    fn index(new_target: Value) -> usize {
        new_target.heap_index().unwrap_or_default() as usize & (LEARNED_RECEIVER_ENTRIES - 1)
    }
}

impl Vm {
    /// Allocates the receiver template `object` of a construction whose body `constructor` runs
    /// first thing, with `constructor` its `new.target`, in the constructor's constant layout or
    /// with its learned storage (`preshape_receiver`).
    #[inline]
    pub(super) fn alloc_construct_receiver(&mut self, object: Object, constructor: Value) -> Value {
        let start = object.shape();
        let this = self.heap.alloc_object(object);
        self.preshape_receiver(this, start, constructor, constructor);
        this
    }

    /// Gives `this`, a just-allocated empty receiver of template shape `start` that no code has
    /// seen, whose construction runs `constructor`'s body next, the layout that body's first
    /// defines make (the module's invariant), and the storage `new_target`'s receivers were
    /// learned to reach (`reserve_learned_receiver`); a constructor with no layout leaves its
    /// shape as it is.
    #[inline]
    pub(super) fn preshape_receiver(&mut self, this: Value, start: u32, constructor: Value, new_target: Value) {
        let index = PreshapedReceiver::index(constructor, start);
        let entry = &self.preshaped_receivers[index];
        if entry.constructor != constructor || entry.start != start {
            self.preshaped_receivers[index] = self.derive_preshaped_receiver(constructor, start);
        }
        let len = usize::from(self.preshaped_receivers[index].len);
        let reserved = self.reserve_learned_receiver(this, new_target, len);
        if len == 0 {
            return;
        }
        if !(reserved && self.append_preshaped_fields(this, index)) {
            // Storage sized for the whole layout at once: no inline-to-arena move or regrowth
            // as the fields arrive.
            let entry = &self.preshaped_receivers[index];
            self.heap.initialize_object_properties(this, entry.shape, &entry.values[..len]);
        }
        // As each add would (`create_shape_data_property`), though a fresh receiver is no
        // prototype any cached method was found on.
        for slot in 0..len {
            let atom = self.preshaped_receivers[index].atoms[slot];
            if self.method_caches_may_name(atom) {
                self.invalidate_method_caches_for_prototype_add(this, atom);
            }
        }
    }

    /// Whether `receiver`, a base class constructor's fresh receiver at the run of leading constant
    /// fields its body defines first (`ConstantFieldsHeld`), already holds that run as its defines
    /// would leave it, so the run's skip is taken (the module's skipped run): its exact shape is
    /// the one the run's first define, through cache site `site`, found already holding its name
    /// (`field_define_overwrite`), as only a receiver made in the constant layout does. One cache
    /// entry and one shape are read; anything else, an empty entry included, answers `false`.
    #[inline(always)]
    pub fn constant_fields_held(&self, receiver: Value, site: u16) -> bool {
        let Some(record_id) = receiver.direct_object_index() else {
            return false;
        };
        let entry = self.field_cache_entry(site);
        let held = entry.holder == NO_FIELD_HOLDER
            && entry.receiver != NO_FIELD_RECEIVER
            && entry.receiver == self.heap.object_record_by_id(record_id).exact_shape();
        #[cfg(test)]
        if held {
            super::call_out::counts::bump(&super::call_out::counts::CONSTANT_FIELD_SKIPS);
        }
        held
    }

    /// Appends the layout of entry `index` to `this`, its receiver still empty over the run
    /// reserved for it (`reserve_learned_receiver`), value by value along the entry's shapes,
    /// as its defines would add them; `false` where an append finds no room, leaving the rest
    /// to `initialize_object_properties`, which replaces whatever storage it holds.
    fn append_preshaped_fields(&mut self, this: Value, index: usize) -> bool {
        let Some(record_id) = this.direct_object_index() else {
            return false;
        };
        let entry = &self.preshaped_receivers[index];
        let heap = &mut self.heap;
        (0..usize::from(entry.len)).all(|slot| heap.record_property_append_reserved(record_id, slot, entry.values[slot], entry.shapes[slot]))
    }

    /// Gives `this`, a just-allocated receiver of a construction with `new_target` that no code
    /// has seen, a young run reserved for the values `new_target`'s receivers reach, where they
    /// reach more than its inline slots hold and than the `preshaped` values its layout gives
    /// it (`Heap::reserve_receiver_run`; the module's slack tracking); while the cycle samples,
    /// first learns from the previous receiver. Its shape and properties stay as they are (still
    /// none: the layout is given after); whether the run was reserved.
    #[inline]
    fn reserve_learned_receiver(&mut self, this: Value, new_target: Value, preshaped: usize) -> bool {
        let index = LearnedReceiver::index(new_target);
        let mut entry = self.learned_receivers[index];
        if entry.new_target != new_target || entry.samples < LEARNED_SAMPLES {
            entry = self.sample_learned_receiver(index, this, new_target);
        }
        let slots = usize::from(entry.slots);
        if slots <= INLINE_PROPERTY_COUNT.max(preshaped) {
            return false;
        }
        if self.heap.reserve_receiver_run(this, entry.template, entry.header, slots) {
            return true;
        }
        self.reserve_learned_template(index, this, slots)
    }

    /// `reserve_learned_receiver` past its entry's template, which `this` does not have: the
    /// first reservation for `new_target`, or one whose `prototype` changed. Derives the entry's
    /// template and header from `this` where it is such a template, and reserves by them.
    #[cold]
    #[inline(never)]
    fn reserve_learned_template(&mut self, index: usize, this: Value, slots: usize) -> bool {
        let Some(data) = self.object_data(this) else { return false };
        let (layout, prototype, shape) = (data.layout_id(), data.proto(), data.shape());
        if layout == self.learned_receivers[index].template
            || data.inline_properties().is_none()
            || data.has_extras()
            || data.has_exotic_access()
            || self.shape_is_dictionary(shape)
            || self.shapes[shape as usize].storage_len() != 0
        {
            // A nursery without the room, or no template.
            return false;
        }
        let header = InlineObjectHeader::of_run(prototype, shape);
        let entry = &mut self.learned_receivers[index];
        entry.template = layout;
        entry.header = header;
        self.heap.reserve_receiver_run(this, layout, header, slots)
    }

    /// The entry of `new_target`, made where another held its index, after one more sample: the
    /// storage length of its previous receiver, which its construction has completed unless
    /// constructions of `new_target` nest. The first sample of a young cycle replaces the
    /// count and the next ones raise it, so the count follows what the constructor builds now.
    #[cold]
    #[inline(never)]
    fn sample_learned_receiver(&mut self, index: usize, this: Value, new_target: Value) -> LearnedReceiver {
        let mut entry = self.learned_receivers[index];
        if entry.new_target != new_target {
            entry = LearnedReceiver { new_target, ..LearnedReceiver::EMPTY };
        }
        let last = std::mem::replace(&mut entry.last, this);
        if let Some(observed) = self.receiver_storage_len(last) {
            let observed = observed.min(LEARNED_SLOTS_MAX) as u8;
            entry.slots = if entry.samples == 0 { observed } else { entry.slots.max(observed) };
            entry.samples += 1;
        }
        self.learned_receivers[index] = entry;
        entry
    }

    /// How many values `receiver`'s storage holds, outside dictionary storage; `None` for
    /// anything else (no object, as `undefined` after a collection).
    fn receiver_storage_len(&self, receiver: Value) -> Option<usize> {
        let data = self.object_data(receiver)?;
        if data.properties().is_dictionary() {
            return None;
        }
        let shape = data.shape();
        (!self.shape_is_dictionary(shape)).then(|| self.shapes[shape as usize].storage_len())
    }

    /// The count `new_target`'s receivers were learned to reach, while its entry holds.
    #[cfg(test)]
    pub(super) fn learned_receiver_slots(&self, new_target: Value) -> Option<u8> {
        let entry = &self.learned_receivers[LearnedReceiver::index(new_target)];
        (entry.new_target == new_target).then_some(entry.slots)
    }

    /// Forgets every learned receiver and constant layout: the heap they name is gone (a reset
    /// or a restored snapshot), so a sampled receiver's record or a layout's string may now be
    /// free.
    pub(super) fn forget_learned_receivers(&mut self) {
        self.learned_receivers.fill(LearnedReceiver::EMPTY);
        self.preshaped_receivers.fill(PreshapedReceiver::EMPTY);
    }

    /// Restarts every learned receiver's sampling for the next young cycle: the receivers
    /// `last` names may be gone. Counts and templates stay as hints until resampled.
    pub(super) fn forget_learned_receiver_samples(&mut self) {
        for entry in self.learned_receivers.iter_mut() {
            entry.last = Value::UNDEFINED;
            entry.samples = 0;
        }
    }

    /// The memo entry of a construction by `constructor` from shape `start`: the constant
    /// layout of the base class whose constructor it is (`Function::constant_fields`, at most
    /// `PRESHAPED_FIELDS` of them: a prefix of the leading run is one too), else its program's
    /// declared layout, the shapes its names step through created (as its defines would create
    /// them) and each new slot seeded from the field's declared type, as the define that creates
    /// it seeds it (`define_field_cached`).
    #[cold]
    #[inline(never)]
    fn derive_preshaped_receiver(&mut self, constructor: Value, start: u32) -> PreshapedReceiver {
        let none = PreshapedReceiver::none(constructor, start);
        let Some(FunctionRecord { kind: FunctionKind::User(program_id, id), .. }) = self.heap.function(constructor) else {
            return none;
        };
        let (program_id, id) = (*program_id, *id);
        if self.shape_is_dictionary(start) || self.shapes[start as usize].storage_len() != 0 {
            return none;
        }
        let fields = {
            let Some(program) = self.programs.residual(program_id) else { return none };
            let Some(body) = program.function_get(id as usize) else { return none };
            // A derived constructor binds no receiver of its own; its base's construction makes it.
            if !body.is_class_constructor || body.derived_constructor {
                return none;
            }
            let declared = program.declared_types.as_deref();
            let class_start = body.source_span.map(|span| span.start);
            let seed = |name: &str| declared_slot_seed(declared.zip(class_start).and_then(|(declared, class)| declared.declared_field(class, name)));
            if !body.constant_fields.is_empty() {
                let mut fields = Vec::with_capacity(body.constant_fields.len().min(PRESHAPED_FIELDS));
                for field in body.constant_fields.iter().take(PRESHAPED_FIELDS) {
                    // A program's atom ids are the VM's: the defines name these very atoms.
                    let Some(name) = program.atoms.get(field.atom as usize) else { return none };
                    if let FieldConstant::String(text) = field.value
                        && program.atoms.get(text as usize).is_none()
                    {
                        return none;
                    }
                    fields.push((LayoutName::Atom(field.atom), field.value, seed(name)));
                }
                fields
            } else {
                let Some(names) = declared.zip(class_start).and_then(|(declared, class)| declared.declared_layout(class)) else { return none };
                if names.is_empty() || names.len() > PRESHAPED_FIELDS {
                    return none;
                }
                names.iter().map(|name| (LayoutName::Text(name.clone()), FieldConstant::Undefined, seed(&**name))).collect::<Vec<_>>()
            }
        };
        let mut entry = none;
        let mut shape = start;
        for (slot, (name, value, seed)) in fields.into_iter().enumerate() {
            let atom = match name {
                LayoutName::Atom(atom) => atom,
                LayoutName::Text(name) => self.intern_atom(&name),
            };
            if entry.atoms[..slot].contains(&atom) || self.is_private_name(atom) {
                return none;
            }
            shape = self.transition_shape(shape, atom);
            if self.shape_is_dictionary(shape) || self.shapes[shape as usize].storage_len() != slot + 1 {
                return none;
            }
            let initial = self.field_constant_value(value);
            if let Some(ty) = seed {
                self.heap.seed_declared_slot(shape, slot, ty, initial);
            }
            entry.atoms[slot] = atom;
            entry.shapes[slot] = shape;
            entry.values[slot] = initial;
            entry.len = slot as u8 + 1;
        }
        entry.shape = shape;
        entry
    }

    /// The value constant field value `value` stands for; a string's is its atom's string, which
    /// the VM keeps for a program atom (`atom_string`).
    fn field_constant_value(&mut self, value: FieldConstant) -> Value {
        match value {
            FieldConstant::Undefined => Value::UNDEFINED,
            FieldConstant::Null => Value::NULL,
            FieldConstant::Boolean(true) => Value::TRUE,
            FieldConstant::Boolean(false) => Value::FALSE,
            FieldConstant::Number(number) => Value::number(number),
            FieldConstant::String(atom) => self.atom_string(atom),
        }
    }
}

/// How a layout names a field: by the atom its defines name, or (a declared layout) by its text.
enum LayoutName {
    Atom(Atom),
    Text(Box<str>),
}

/// The slot type seed of a field declared with language bits `bits`, as `declared_field_type`
/// derives it: `None` for no declaration, an open or empty set, or a set whose slot class is
/// `SlotType::Tagged`.
fn declared_slot_seed(bits: Option<u16>) -> Option<crate::typed_ir::Type> {
    let bits = bits?;
    if bits == crate::typed_ir::ANY_LANGUAGES || bits == crate::typed_ir::Languages::EMPTY.bits() {
        return None;
    }
    let ty = crate::typed_ir::Type::languages(bits);
    (crate::typed_ir::SlotType::from_type(ty) != crate::typed_ir::SlotType::Tagged).then_some(ty)
}
