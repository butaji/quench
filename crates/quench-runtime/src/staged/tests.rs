use super::*;
use crate::heap::{Cell, Heap, Object};
use crate::value_vec::INLINE_PROPERTY_COUNT;
use crate::Value;

/// The storage forms an exact-shape object can take, by how the interpreter's object API
/// (independent of `staged`) describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Observed {
    Packed,
    Arena,
    DisplacedInline,
    DisplacedArena,
}

fn observed(object: &Object) -> Observed {
    match (object.inline_properties().is_some(), object.plain_property_storage().is_some()) {
        (true, true) => Observed::Packed,
        (false, true) => Observed::Arena,
        (true, false) => Observed::DisplacedInline,
        (false, false) => Observed::DisplacedArena,
    }
}

/// Objects of every storage form, each with its values: narrow integers, narrow references,
/// a double (no narrow form), and enough values to spill past the inline ones.
fn objects_of_every_form(heap: &mut Heap) -> Vec<(Value, Vec<Value>)> {
    let wide = INLINE_PROPERTY_COUNT + 2;
    for shape in 1..=wide {
        heap.register_property_shape(shape as u32, shape, None);
    }
    let reference = heap.alloc(Cell::Error("reference".into()));
    let value_sets: Vec<Vec<Value>> = vec![
        vec![Value::number(7.0), Value::number(-3.0)],
        vec![reference, Value::TRUE],
        vec![Value::number(0.5), Value::NULL],
        (0..wide).map(|index| Value::number(index as f64)).collect(),
    ];
    let mut objects = Vec::new();
    for prototype in [false, true] {
        for values in &value_sets {
            let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
            for (slot, value) in values.iter().enumerate() {
                heap.property_add(object, *value, slot as u32 + 1);
            }
            if prototype {
                heap.object_record_mut(object).unwrap().mark_as_prototype();
            }
            objects.push((object, values.clone()));
        }
    }
    objects
}

#[test]
fn the_interpreter_reading_of_own_layout_matches_the_object_model_in_every_form() {
    let mut heap = Heap::new();
    let objects = objects_of_every_form(&mut heap);
    let mut forms = std::collections::HashSet::new();
    for (object, values) in objects {
        let data = heap.object(object).unwrap();
        forms.insert(observed(data));
        for (slot, expected) in values.iter().enumerate() {
            // SAFETY: the object's exact shape holds every slot it was given.
            let reference = unsafe { heap.property_get_unchecked(data, slot) };
            assert_eq!(reference, *expected, "{:?} slot {slot}", observed(data));
            // SAFETY: as above; the heap's arena holds the object's run.
            let staged = unsafe { concrete_own_read(data, heap.property_values_base(), slot) };
            assert_eq!(staged, *expected, "{:?} slot {slot}", observed(data));
        }
    }
    for form in [Observed::Packed, Observed::Arena, Observed::DisplacedInline, Observed::DisplacedArena] {
        assert!(forms.contains(&form), "the corpus covers {form:?}: {forms:?}");
    }
}

#[test]
fn a_narrow_decode_inverts_every_narrow_encoding() {
    let mut heap = Heap::new();
    let reference = heap.alloc(Cell::Error("reference".into()));
    let limit = 1_i32 << (NarrowLayout::INTEGER_BITS - 1);
    let values = [Value::number(0.0), Value::number(f64::from(limit - 1)), Value::number(f64::from(-limit)),
        Value::UNDEFINED, Value::NULL, Value::TRUE, Value::FALSE, Value::DELETED, reference];
    let mut stage = Concrete::new(std::ptr::null());
    for value in values {
        let narrow = value.to_narrow().unwrap_or_else(|| panic!("{value:?} has a narrow form"));
        let decoded = decode_narrow(&mut stage, u64::from(narrow));
        assert_eq!(Value::from_bits(decoded), Value::from_narrow(narrow), "{value:?}");
        assert_eq!(Value::from_bits(decoded), value);
    }
}

/// Hand-written decodes of the object and narrow layouts outside `staged`, per native-tier
/// file: each is a second home of a rule `staged` owns (AGENTS.md, manifesto 4). The counts
/// only go down; a new decode belongs in `staged` as an operation every tier runs.
const LAYOUT_DECODES: &[(&str, &str, usize)] = &[
    ("vm/compiled.rs", include_str!("../vm/compiled.rs"), 9),
    ("vm/compiled/lane_region.rs", include_str!("../vm/compiled/lane_region.rs"), 5),
    ("vm/compiled/osr_loop.rs", include_str!("../vm/compiled/osr_loop.rs"), 1),
    ("vm/compiled/object_slots.rs", include_str!("../vm/compiled/object_slots.rs"), 2),
    ("vm/js_compiled.rs", include_str!("../vm/js_compiled.rs"), 0),
    ("vm/dispatch_fast.rs", include_str!("../vm/dispatch_fast.rs"), 0),
];

/// The layout facts whose use is a decode: the bits a reader tests to learn a storage form or
/// a narrow value's meaning.
const LAYOUT_TOKENS: &[&str] = &[
    "extras_tag_mask",
    "inline_storage_mask",
    "storage_start_mask",
    "NarrowLayout::INTEGER_FLAG",
    "NarrowLayout::CODE_SHIFT",
];

#[test]
fn staged_coverage_layout_decodes_outside_staged_only_shrink() {
    for (file, source, allowed) in LAYOUT_DECODES {
        let found: usize = LAYOUT_TOKENS.iter().map(|token| source.matches(token).count()).sum();
        assert!(found <= *allowed,
            "{file} decodes object or narrow layouts {found} times (allowed {allowed}): route the read through `staged`");
        assert!(found >= *allowed,
            "{file} now decodes layouts {found} times, below {allowed}: lower its allowance in LAYOUT_DECODES");
    }
}

/// The interpreters: the one place an opcode's meaning is written by hand.
const INTERPRETERS: &[&str] = &["vm/dispatch.rs", "vm/dispatch_wasm.rs"];

/// Opcode match arms per file and opcode (`Op::X` before a `=>`) in `vm/`, outside the
/// interpreters and tests.
fn opcode_arms() -> std::collections::BTreeMap<(String, String), usize> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pending = vec![root.join("vm")];
    let mut arms = std::collections::BTreeMap::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); continue; }
            let file = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.ends_with(".rs") || name.ends_with("tests.rs") || INTERPRETERS.contains(&file.as_str()) { continue; }
            for line in std::fs::read_to_string(&path).unwrap().lines() {
                let Some((pattern, _)) = line.split_once("=>") else { continue };
                for (at, _) in pattern.match_indices("Op::") {
                    let op: String = pattern[at + 4..].chars().take_while(char::is_ascii_alphanumeric).collect();
                    if op.starts_with(|c: char| c.is_ascii_uppercase()) {
                        *arms.entry((file.clone(), op)).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    arms
}

/// Futamura's first projection, enforced: a tier's meaning of an opcode is the interpreter's,
/// specialized (`staged`, semantic entries), so an interpreter change reaches every tier. An
/// opcode meaning a tier writes by hand is a second definition that can drift; the table of
/// them only shrinks, and a new one fails here.
#[test]
fn tiers_derive_opcode_meaning_from_the_interpreter() {
    let allowed: std::collections::BTreeMap<(String, String), usize> = include_str!("hand_semantics.txt").lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split(' ');
            let (file, op, count) = (fields.next().unwrap(), fields.next().unwrap(), fields.next().unwrap());
            ((file.to_owned(), op.to_owned()), count.parse().unwrap())
        })
        .collect();
    let found = opcode_arms();
    for ((file, op), count) in &found {
        let limit = allowed.get(&(file.clone(), op.clone())).copied().unwrap_or(0);
        assert!(*count <= limit, "{file} hand-writes Op::{op} {count} times (allowed {limit}): derive it from the \
            interpreter through `staged` or a semantic entry instead");
    }
    for ((file, op), limit) in &allowed {
        let count = found.get(&(file.clone(), op.clone())).copied().unwrap_or(0);
        assert!(count >= *limit, "{file} now hand-writes Op::{op} {count} times: lower its line in \
            staged/hand_semantics.txt to {count} (or drop it at 0)");
    }
}
