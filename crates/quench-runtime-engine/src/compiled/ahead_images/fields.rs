//! The field reads an ahead lane body guesses the shape of from the closed world, where a JIT body
//! reads its site's observed shapes (`Vm::unit_fields_in_lane`): a read of name `atom` guesses the
//! constructed functions whose bodies store a field of that name into `this` (`this_fields`), up to
//! `GUESSED_SHAPES` of them, each as the shape its construction makes (`ShapeRecipe`: the function's `prototype`, then those
//! fields in store order), and reads the field's slot in it. The guard names the shape by its
//! symbol (`Symbol::GuardShape`), which the loader resolves to the shape's id, or to an id no
//! record has where the VM builds none or the slot's words are not tagged values. A guess is
//! never an observation: a miss asks the VM's caches (`NativeFieldLoad::probes_misses`), so a wrong
//! guess costs a compare, never a leave. A store of the name takes the same shapes, with the one
//! class of value every store of the name in the program writes (`AotFacts::field_type`): an int32,
//! a direct object or `null`, or any tagged word. The loader admits the slot to that class, so the
//! store is one arm whose slot holds what it writes, and a miss takes the VM's store as before. A
//! read of a field of one class reads it as that class (`GuardAccess::ReadInt32`,
//! `GuardAccess::ReadReference`) under the slot's dependency, the fact proven where the slot is
//! written: its miss leaves (`NativeFieldLoad::probes_misses`), so the value it defines keeps it.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::FieldLookup;
use crate::aot_image::symbols::{GuardAccess, HeapValueRecipe, ShapeRecipe};
use super::constructions::ADDED_FIELD;

/// The guessed loads of `unit`'s named field reads, by field-cache site (`lane_region::FieldSites`).
pub(crate) fn guessed_fields(program: &ResidualProgram, unit: &lane_unit::Unit, facts: &AotFacts, symbols: &mut SymbolTable)
    -> Vec<Option<Vec<NativeFieldLoad>>> {
    let owners = field_owners(program, facts);
    let inherited = prototype_owners(facts, &owners);
    if owners.is_empty() && inherited.is_empty() { return Vec::new(); }
    let sites: Vec<(usize, Vec<NativeFieldLoad>)> = unit.instructions.iter().flat_map(|instruction| accessed(program, *instruction)).filter_map(|(atom, site, store)| {
            // A name both constructions store and prototypes hold (a task's `queue` field, the
            // scheduler's `queue` method) guesses both: a read of either receiver finds its arm.
            let (read, stored) = GuardAccess::of_field(facts.field_type(atom));
            let access = if store { stored } else { read };
            let own = owners.get(&atom).map(|owners| owners.iter()
                .map(|&owner| guessed_load(program, owner, access, &mut *symbols)).collect::<Option<Vec<_>>>());
            let held = inherited.get(&atom).filter(|_| !store).map(|holders| holders.iter()
                .map(|&(function, slot, len)| guessed_holder_load(program, (function, slot, len), atom, &mut *symbols)).collect::<Option<Vec<_>>>());
            let loads: Vec<NativeFieldLoad> = match (own, held) {
                (None, None) => return None,
                (own, held) => own.unwrap_or(Some(Vec::new()))?.into_iter().chain(held.unwrap_or(Some(Vec::new()))?).collect(),
            };
            if loads.len() > GUESSED_SHAPES { return None; }
            if loads.is_empty() { return None; }
            Some((usize::from(site), loads))
        }).collect();
    if quench_runtime::trace::Channel::AotLanes.on() {
        let unguessed: Vec<_> = unit.instructions.iter().flat_map(|instruction| accessed(program, *instruction))
            .filter(|(atom, _, _)| !owners.contains_key(atom) && !inherited.contains_key(atom)).filter_map(|(atom, _, _)| program.atoms.get(atom as usize)).collect();
        eprintln!("aot-lanes: guesses {} field reads ({} own names, {} prototype names), not {unguessed:?}", sites.len(), owners.len(), inherited.len());
    }
    lane_region::flat_table(sites)
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

/// The most constructed functions a read guesses its receiver is made by: a family of shapes behind
/// one guard each (`object_slots::field_load_groups`).
const GUESSED_SHAPES: usize = 8;

/// By field name, the constructed functions that store a field of that name into `this`, each with
/// the field's slot in the shape its construction makes; a name more than `GUESSED_SHAPES` such
/// functions store is guessed by none.
fn field_owners(program: &ResidualProgram, facts: &AotFacts) -> BTreeMap<u32, Vec<(u32, u16)>> {
    let mut owners: BTreeMap<u32, Vec<(u32, u16)>> = BTreeMap::new();
    for function in facts.constructed_functions() {
        for (slot, atom) in this_fields(program, function).into_iter().enumerate() {
            let Ok(slot) = u16::try_from(slot) else { break };
            owners.entry(atom).or_default().push((function, slot));
        }
    }
    owners.retain(|_, owners| owners.len() <= GUESSED_SHAPES);
    owners
}

/// By method name, the constructed functions whose `prototype` the program gives that name
/// (`AotFacts::prototype_fields`), each with the slot the name takes in the prototype after its
/// `constructor` and the prototype's field count; a name more than `GUESSED_SHAPES` prototypes and
/// constructions take together is guessed by none.
fn prototype_owners(facts: &AotFacts, owners: &BTreeMap<u32, Vec<(u32, u16)>>) -> BTreeMap<u32, Vec<(u32, u16, usize)>> {
    let mut holders: BTreeMap<u32, Vec<(u32, u16, usize)>> = BTreeMap::new();
    for function in facts.constructed_functions() {
        let fields = facts.prototype_fields(function);
        for (index, &atom) in fields.iter().enumerate() {
            let Ok(slot) = u16::try_from(index + 1) else { break };
            holders.entry(atom).or_default().push((function, slot, fields.len() + 1));
        }
    }
    holders.retain(|atom, holders| holders.len() + owners.get(atom).map_or(0, Vec::len) <= GUESSED_SHAPES);
    holders
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
fn guessed_holder_load(program: &ResidualProgram, (function, slot, len): (u32, u16, usize), atom: u32, symbols: &mut SymbolTable) -> Option<NativeFieldLoad> {
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
fn guessed_load(program: &ResidualProgram, (function, slot): (u32, u16), access: GuardAccess, symbols: &mut SymbolTable) -> Option<NativeFieldLoad> {
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
