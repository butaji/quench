use super::super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

#[test]
fn moved_shape_lookup_index_matches_the_derived_index() {
    // A wide object built key by key, with non-default attributes, redefinitions and
    // deletions, keeps
    // its lookup index moving to each new shape; every index must match the one derived
    // from that shape's whole chain.
    let source = r#"
      var wide = {};
      for (var i = 0; i < 300; i++) {
        wide["k" + i] = i;
        if (i % 7 === 0) wide["k" + i];
        if (i % 5 === 0) Object.defineProperty(wide, "k" + i, { enumerable: false });
        if (i % 11 === 0) Object.defineProperty(wide, "g" + i, { get() { return i; } });
        if (i % 3 === 0) delete wide["k" + (i - 1)];
      }
      var keys = Object.keys(wide).length;
    "#;
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize(source, "wide.js").unwrap();
    vm.execute(&program).unwrap();
    let mut compared = 0;
    for shape in 0..vm.shapes.len() {
        let Some(index) = vm.shapes[shape].lookup_index.get() else {
            continue;
        };
        let derived = super::derive_shape_lookup_index(&vm.shapes, shape as u32);
        // A moved index keeps deleted entries vacant until compaction; its live entries,
        // in order, are the derived ones.
        assert!(index.live_entries().eq(derived.live_entries()), "shape {shape} entries");
        assert!(index.attributes == derived.attributes, "shape {shape} attributes");
        compared += 1;
    }
    assert!(compared > 0, "the wide object's shapes keep an index");
}


#[test]
fn a_map_like_object_keeps_key_order_and_values_through_vacated_index_entries() {
    // Adds and deletes in turn move the object to a dictionary shape whose index marks
    // deleted keys vacant and compacts them; enumeration, lookup and `in` must stay exact.
    let source = r#"
      var map = {};
      var model = new Map();
      for (var i = 0; i < 4000; i++) {
        var key = "d" + (i & 255);
        map[key] = i; model.set(key, i);
        if (i & 1) { var gone = "d" + ((i * 7) & 255); delete map[gone]; model.delete(gone); }
      }
      var keys = Object.keys(map).join(",");
      var expected = Array.from(model.keys()).join(",");
      var values = 0, missing = 0;
      for (var k in map) values += map[k];
      model.forEach(function (v, k) { if (!(k in map) || map[k] !== v) missing++; });
      var total = 0; model.forEach(function (v) { total += v; });
      print([keys === expected, values === total, missing, "d3" in map === model.has("d3")].join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "map-like.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["true true 0 true"]);
}


#[test]
fn objects_leaving_a_shared_dictionary_shape_reuse_its_transitions() {
    // Boxing a string adds one property per character, so boxing a string longer than the
    // property-count trigger turns the shared box layout into a dictionary partway. Every
    // later box of that string must take the cached transitions from there, not append
    // shapes of its own (SunSpider tagcloud slowed to a crawl when they did).
    let shapes_after = |boxes: usize| {
        let source = format!(
            "var text = 'x'.repeat(66000); var n = 0;
             for (var i = 0; i < {boxes}; i++) n += Object(text).length;"
        );
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&Engine::specialize(&source, "box.js").unwrap()).unwrap();
        vm.shapes.len()
    };
    let growth = shapes_after(3) - shapes_after(1);
    assert!(growth < 16, "boxing appended {growth} shapes");
}
