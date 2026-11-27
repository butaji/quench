//! The field reads and stores an ahead lane body loads by the classes the solver names for them
//! (`FunctionFacts::access_classes`: the closed set of classes of the object each access reads or
//! stores). Each class of the set contributes its layout, as its construction makes the shape
//! (`ShapeRecipe`: the function's `prototype`, then the fields it stores into `this`, in store order):
//! the slot of the field in it (`own_load`), or, for a read of a method its `prototype` holds, the
//! slot of the name there (`holder_load`). The guard names each shape by its symbol (`Symbol::GuardShape`),
//! which the loader resolves to the shape's id, or to an id no record has where the VM builds none or
//! the slot's words are not tagged values. A miss asks the VM's caches (`NativeFieldLoad::probes_misses`),
//! so a wrong layout costs a compare, never a leave. A store takes the class's shape with the one class
//! of value the stores of that field into that class write (`AotFacts::field_type_on`): an int32, a direct
//! object or `null`, or any tagged word. The loader admits the slot to that class, so the store is one arm
//! whose slot holds what it writes, and a miss takes the VM's store as before. A read of a field of one
//! class reads it as that class (`GuardAccess::ReadInt32`, `GuardAccess::ReadReference`) under the slot's
//! dependency, the fact proven where the slot is written: its miss leaves (`NativeFieldLoad::probes_misses`),
//! so the value it defines keeps it.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::FieldLookup;
use crate::aot_image::symbols::{GuardAccess, HeapValueRecipe, ShapeRecipe};
use super::constructions::ADDED_FIELD;

/// The most shape guards one field-cache site's loads take: a family of shapes behind one guard each
/// (`object_slots::field_load_groups`). A site over the bound keeps no load and asks the VM's caches.
const SHAPES_PER_SITE: usize = 8;

/// The loads of `unit`'s named field reads and stores, by field-cache site (`lane_region::FieldSites`):
/// each access the solver names a closed set of classes for takes the loads of every class of the set
/// (`class_loads`). An access the solver names no set for, or one of a site that reads two objects, takes none.
pub(crate) fn class_fields(program: &ResidualProgram, (unit, function): (&lane_unit::Unit, u32), facts: &AotFacts, symbols: &mut SymbolTable)
    -> Vec<Option<Vec<NativeFieldLoad>>> {
    let sites: Vec<(usize, Vec<NativeFieldLoad>)> = unit.instructions.iter().zip(&unit.origins).flat_map(|(instruction, origin)| {
            // A nested lookup's reads have more than one object, so only a single access takes the classes of its origin.
            let accesses = accessed(program, *instruction);
            let classes = if accesses.len() == 1 { access_classes((unit, function), facts, *origin) } else { Vec::new() };
            accesses.into_iter().map(move |access| (access, classes.clone()))
        }).filter_map(|((atom, site, store), classes)| {
            let mut loads = Vec::new();
            for &class in &classes { loads.extend(class_loads(program, facts, &mut *symbols, class, atom, store)); }
            if loads.is_empty() || loads.len() > SHAPES_PER_SITE { return None; }
            Some((usize::from(site), loads))
        }).collect();
    if quench_runtime::trace::Channel::AotLanes.on() {
        eprintln!("aot-lanes: class loads for {} field-cache sites", sites.len());
    }
    lane_region::flat_table(sites)
}

/// The classes the solver names for the object the named field access at `origin` of `unit` reads or
/// stores (`FunctionFacts::access_classes`), in the function the instruction is from; none where it names no closed set.
fn access_classes((unit, function): (&lane_unit::Unit, u32), facts: &AotFacts, origin: lane_unit::Origin) -> Vec<u32> {
    let (function, pc) = match origin {
        lane_unit::Origin::Caller(pc) => (function, pc),
        lane_unit::Origin::Inlined { frame, pc } => match unit.frames.get(frame) {
            Some(frame) => (frame.target.function, pc),
            None => return Vec::new(),
        },
    };
    facts.function_facts(function).access_classes(pc).ids().to_vec()
}

/// The name, field-cache site and kind (a store) of each named field access of `instruction`: a
/// read by name; each read of a nested site (`FieldLookup::Site`: `this.currentTcb.link`), the
/// first and the second, and the store into the current `this` it sinks to; or a named store
/// (`Operation::is_field_store`).
fn accessed(program: &ResidualProgram, instruction: WideInstruction) -> Vec<(u32, u16, bool)> {
    use quench_runtime::staged::Operation;
    if Operation::<u64, ()>::is_field_read(instruction) {
        return match instruction.field_lookup() {
            Some(FieldLookup::Atom { atom, cache_site, .. }) => vec![(atom, cache_site, false)],
            Some(FieldLookup::Site(index)) => program.field_sites.get(index).map(|site| {
                let reads = [Some(site.first), site.second].into_iter().flatten().map(|(atom, cache)| (atom, cache, false));
                let sink = site.sink.filter(|_| instruction.writes_current_this()).map(|(atom, cache)| (atom, cache, true));
                reads.chain(sink).collect()
            }).unwrap_or_default(),
            None => Vec::new(),
        };
    }
    if Operation::<u64, ()>::is_field_store(instruction) { vec![(instruction.atom_index(), instruction.cache_site_index(), true)] } else { Vec::new() }
}

/// The loads of field `atom` (a store where `store`) of the objects of class `class` (a constructed
/// function): the slot of its own field in the shape its construction makes (`own_load`), and, for a
/// read, the slot its `prototype` holds the name at (`holder_load`).
fn class_loads(program: &ResidualProgram, facts: &AotFacts, symbols: &mut SymbolTable, class: u32, atom: u32, store: bool) -> Vec<NativeFieldLoad> {
    let mut loads = Vec::new();
    let fields = this_fields(program, class);
    if let Some(slot) = fields.iter().position(|&field| field == atom).and_then(|slot| u16::try_from(slot).ok()) {
        let (read, stored) = GuardAccess::of_field(facts.field_type_on(class, atom));
        loads.extend(own_load(program, (class, slot), if store { stored } else { read }, symbols));
    }
    let held = facts.prototype_fields(class);
    if let Some(slot) = held.iter().position(|&name| name == atom).and_then(|index| u16::try_from(index + 1).ok()).filter(|_| !store) {
        loads.extend(holder_load(program, (class, slot, held.len() + 1), atom, symbols));
    }
    loads
}

/// The names of the fields `function`'s body stores into `this`, each once, in the order its
/// stores first name them: the shape a construction that runs every store makes.
fn this_fields(program: &ResidualProgram, function: u32) -> Vec<u32> {
    let Some(body) = program.function_get(function as usize) else { return Vec::new() };
    let mut fields: Vec<u32> = Vec::new();
    for instruction in body.instructions().filter(|instruction| instruction.stores_to_this()) {
        let atom = instruction.imm();
        if !fields.contains(&atom) { fields.push(atom); }
    }
    fields
}

/// The read of method `atom` of an object a construction of `function` makes, which its
/// `prototype` holds at `slot` of its `len` fields: the receiver's guard names the construction's
/// shape (`GuardAccess::Inherited`), the holder's the prototype's shape where its slot holds the
/// name (`GuardAccess::Holder`), both by symbol; the slot is read live, so a method stored again
/// reads its new value, and a prototype given another layout misses its guard and asks the caches.
fn holder_load(program: &ResidualProgram, (function, slot, len): (u32, u16, usize), atom: u32, symbols: &mut SymbolTable) -> Option<NativeFieldLoad> {
    let fields = this_fields(program, function);
    let Symbol::HeapValue(prototype) = symbols.heap_value(HeapValueRecipe::PrototypeOf(function)) else { return None };
    let Symbol::HeapValue(constructor) = symbols.heap_value(HeapValueRecipe::ClosureOf(function)) else { return None };
    let receiver = ShapeRecipe { prototype: Some(prototype), constructor: Some(constructor), fields: fields.iter().map(|atom| (*atom, ADDED_FIELD)).collect() };
    let holder = ShapeRecipe { prototype: Some(prototype), constructor: None, fields: Box::new([(atom, 0)]) };
    let symbolic = quench_runtime::staged::ic::SYMBOLIC_SHAPE;
    let receiver = symbols.guard_shape(receiver, 0, GuardAccess::Inherited).filter(|key| *key < symbolic)?;
    let holder = symbols.guard_shape(holder, slot, GuardAccess::Holder).filter(|key| *key < symbolic)?;
    Some(NativeFieldLoad {
        shape: symbolic | receiver, holder: Some(symbolic | holder), slot, storage_len: len, storage: None,
        ty: quench_runtime::typed_ir::Type::ANY, inherited: None, function_shape: false, raw_double: false, formed: None, holder_id: None,
    })
}

/// The load of `slot` of the shape a construction of `function` makes, its guard named by symbol:
/// a read's of any value, a store's of the class of values `access` stores (`GuardAccess::stored`).
fn own_load(program: &ResidualProgram, (function, slot): (u32, u16), access: GuardAccess, symbols: &mut SymbolTable) -> Option<NativeFieldLoad> {
    let fields = this_fields(program, function);
    let Symbol::HeapValue(prototype) = symbols.heap_value(HeapValueRecipe::PrototypeOf(function)) else { return None };
    let Symbol::HeapValue(constructor) = symbols.heap_value(HeapValueRecipe::ClosureOf(function)) else { return None };
    let recipe = ShapeRecipe { prototype: Some(prototype), constructor: Some(constructor), fields: fields.iter().map(|atom| (*atom, ADDED_FIELD)).collect() };
    let key = symbols.guard_shape(recipe, slot, access)?;
    (key < quench_runtime::staged::ic::SYMBOLIC_SHAPE).then_some(NativeFieldLoad {
        shape: quench_runtime::staged::ic::SYMBOLIC_SHAPE | key, holder: None, slot, storage_len: fields.len(), storage: None,
        ty: access.stored().or(access.read()).unwrap_or(quench_runtime::typed_ir::Type::ANY), inherited: None, function_shape: false, raw_double: false, formed: None, holder_id: None,
    })
}
