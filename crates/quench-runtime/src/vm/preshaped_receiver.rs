//! Receivers made in their declared layout: a TypeScript base class's construction allocates its
//! receiver already holding the leading fields its constructor defines `undefined` before any
//! code can observe it (`DeclaredLayout`), so those defines find their slots and every instance
//! has the class's layout from birth.
//!
//! Invariant: a receiver of `constructor` made from a template of shape `start` (empty storage)
//! is given the shape that adding the layout's names in order to `start` reaches, each value
//! `undefined`, and nothing else. This is exactly the object the constructor's first
//! `DefineField`s make (`compile::class::emit_instance_fields`): a base class's [[Construct]] runs
//! InitializeInstanceElements right after OrdinaryCreateFromConstructor, before the body and the
//! parameter bindings; a field with no initializer evaluates nothing and defines `undefined` by
//! CreateDataPropertyOrThrow, which on a fresh extensible ordinary object cannot fail and runs no
//! code; and the layout stops at the first field that is not of that kind
//! (`compile::typescript::fields::layout`). Until that field, no code runs and nothing but the
//! constructor's frame holds the receiver, so no `in`, `Object.keys`, `hasOwnProperty`, getter or
//! Proxy can see the properties exist earlier than JS defines them. The defines themselves then
//! find an own writable, enumerable, configurable data property and leave it as it is
//! (CreateDataProperty's value write of `undefined` over `undefined`), keeping the key order.
//! Private methods (marked before the fields) and private fields (after) touch no public key.
//!
//! Any other path that allocates an empty receiver for the same class stays correct: its defines
//! add the fields one by one, reaching the same shape, as before.
use super::*;

/// Entries in the direct-mapped `Vm::preshaped_receivers` memo; a power of two.
pub(super) const PRESHAPED_RECEIVER_ENTRIES: usize = 64;
/// The most fields a layout gives a receiver at its allocation; a longer layout keeps the defines.
pub(super) const PRESHAPED_FIELDS: usize = 16;

/// What a construction by `constructor` from a template of shape `start` allocates: the shape its
/// layout's `len` names (`atoms`) reach from `start`, or nothing more (`len` 0). Cleared by every
/// collection, which may free `constructor` and renumber shapes.
#[derive(Clone, Copy)]
pub(super) struct PreshapedReceiver {
    constructor: Value,
    start: u32,
    shape: u32,
    len: u8,
    atoms: [Atom; PRESHAPED_FIELDS],
}

impl PreshapedReceiver {
    /// An entry no construction matches: no constructor is `undefined`.
    pub(super) const EMPTY: Self = Self {
        constructor: Value::UNDEFINED,
        start: u32::MAX,
        shape: u32::MAX,
        len: 0,
        atoms: [0; PRESHAPED_FIELDS],
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

impl<H: Host> Vm<H> {
    /// Allocates the receiver template `object` of a construction whose body `constructor` runs
    /// first thing, in the constructor's declared layout (`preshape_receiver`).
    #[inline]
    pub(super) fn alloc_construct_receiver(&mut self, object: Object, constructor: Value) -> Value {
        let start = object.shape();
        let this = self.heap.alloc_object(object);
        self.preshape_receiver(this, start, constructor);
        this
    }

    /// Gives `this`, a just-allocated empty receiver of template shape `start` that no code has
    /// seen, whose construction runs `constructor`'s body next, the layout that body's first
    /// defines make (the module's invariant); a constructor with none leaves it as it is.
    #[inline]
    pub(super) fn preshape_receiver(&mut self, this: Value, start: u32, constructor: Value) {
        let index = PreshapedReceiver::index(constructor, start);
        let mut entry = self.preshaped_receivers[index];
        if entry.constructor != constructor || entry.start != start {
            entry = self.derive_preshaped_receiver(constructor, start);
            self.preshaped_receivers[index] = entry;
        }
        if entry.len == 0 {
            return;
        }
        let len = usize::from(entry.len);
        let values = [Value::UNDEFINED; PRESHAPED_FIELDS];
        // Storage sized for the whole layout at once: no inline-to-arena move or regrowth as the
        // fields arrive, and the defines that follow are plain value writes.
        self.heap.initialize_object_properties(this, entry.shape, &values[..len]);
        // As each add would (`create_shape_data_property`), though a fresh receiver is no
        // prototype any cached method was found on.
        for &atom in &entry.atoms[..len] {
            if self.method_caches_may_name(atom) {
                self.invalidate_method_caches_for_prototype_add(this, atom);
            }
        }
    }

    /// The memo entry of a construction by `constructor` from shape `start`: its program's
    /// declared layout for the base class whose constructor it is, the shapes its names step
    /// through created (as its defines would create them) and each new slot seeded from the
    /// field's declared type, as the define that creates it seeds it (`define_field_cached`).
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
            let Some(declared) = program.declared_types.as_deref() else { return none };
            let Some(body) = program.function_get(id as usize) else { return none };
            // A derived constructor binds no receiver of its own; its base's construction makes it.
            if !body.is_class_constructor || body.derived_constructor {
                return none;
            }
            let Some(class_start) = body.source_span.map(|span| span.start) else { return none };
            let Some(names) = declared.declared_layout(class_start) else { return none };
            if names.is_empty() || names.len() > PRESHAPED_FIELDS {
                return none;
            }
            names
                .iter()
                .map(|name| (name.clone(), declared_slot_seed(declared.declared_field(class_start, name))))
                .collect::<Vec<_>>()
        };
        let mut entry = none;
        let mut shape = start;
        for (slot, (name, seed)) in fields.iter().enumerate() {
            let atom = self.intern_atom(name);
            if entry.atoms[..slot].contains(&atom) {
                return none;
            }
            shape = self.transition_shape(shape, atom);
            if self.shape_is_dictionary(shape) || self.shapes[shape as usize].storage_len() != slot + 1 {
                return none;
            }
            if let Some(ty) = *seed {
                self.heap.seed_declared_slot(shape, slot, ty);
            }
            entry.atoms[slot] = atom;
        }
        entry.shape = shape;
        entry.len = fields.len() as u8;
        entry
    }
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
