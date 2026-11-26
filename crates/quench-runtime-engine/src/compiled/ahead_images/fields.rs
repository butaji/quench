//! The field reads an ahead lane body guesses the shape of from the closed world, where a JIT body
//! reads its site's observed shapes (`Vm::unit_fields_in_lane`): a read of name `atom` guesses the
//! constructed functions whose bodies store a field of that name into `this` (`this_fields`), up to
//! `GUESSED_SHAPES` of them, each as the shape its construction makes (`ShapeRecipe`: the function's `prototype`, then those
//! fields in store order), and reads the field's slot in it. The guard names the shape by its
//! symbol (`Symbol::GuardShape`), which the loader resolves to the shape's id, or to an id no
//! record has where the VM builds none or the slot's words are not tagged values. A guess is
//! never an observation: a miss asks the VM's caches (`NativeFieldLoad::probes_misses`), so a wrong
//! guess costs a compare, never a leave.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::FieldLookup;
use crate::aot_image::symbols::{HeapValueRecipe, ShapeRecipe};
use super::constructions::ADDED_FIELD;

/// The guessed loads of `unit`'s named field reads, by field-cache site (`lane_region::FieldSites`).
pub(crate) fn guessed_fields(program: &ResidualProgram, unit: &lane_unit::Unit, facts: &AotFacts, symbols: &mut SymbolTable)
    -> Vec<Option<Vec<NativeFieldLoad>>> {
    let owners = field_owners(program, facts);
    if owners.is_empty() { return Vec::new(); }
    let sites: Vec<(usize, Vec<NativeFieldLoad>)> = unit.instructions.iter().filter(|instruction| quench_runtime::staged::Operation::<u64, ()>::is_field_read(**instruction))
        .filter_map(|instruction| {
            let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup() else { return None };
            let loads = owners.get(&atom)?.iter().map(|&(function, slot)| guessed_load(program, function, slot, symbols)).collect::<Option<Vec<_>>>()?;
            Some((usize::from(cache_site), loads))
        }).collect();
    if quench_runtime::trace::Channel::AotLanes.on() {
        eprintln!("aot-lanes: guesses {} field reads ({} names have few constructors)", sites.len(), owners.len());
    }
    lane_region::flat_table(sites)
}

/// The most constructed functions a read guesses its receiver is made by: a family of shapes behind
/// one guard each (`object_slots::field_load_groups`).
const GUESSED_SHAPES: usize = 4;

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

/// The load of `slot` of the shape a construction of `function` makes, its guard named by symbol.
fn guessed_load(program: &ResidualProgram, function: u32, slot: u16, symbols: &mut SymbolTable) -> Option<NativeFieldLoad> {
    let fields = this_fields(program, function);
    let Symbol::HeapValue(prototype) = symbols.heap_value(HeapValueRecipe::PrototypeOf(function)) else { return None };
    let Symbol::HeapValue(constructor) = symbols.heap_value(HeapValueRecipe::ClosureOf(function)) else { return None };
    let recipe = ShapeRecipe { prototype: Some(prototype), constructor: Some(constructor), fields: fields.iter().map(|atom| (*atom, ADDED_FIELD)).collect() };
    let key = symbols.guard_shape(recipe, slot)?;
    (key < quench_runtime::staged::ic::SYMBOLIC_SHAPE).then_some(NativeFieldLoad {
        shape: quench_runtime::staged::ic::SYMBOLIC_SHAPE | key, holder: None, slot, storage_len: fields.len(), storage: None,
        ty: quench_runtime::typed_ir::Type::ANY, inherited: None, function_shape: false, raw_double: false, formed: None, holder_id: None,
    })
}
