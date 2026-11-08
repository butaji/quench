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
    // Objects built by the same adds share one layout, which turns dictionary partway past
    // the property-count trigger. Every later object must take the cached transitions from
    // there, not append shapes of its own (SunSpider tagcloud stalled when boxed strings,
    // which then held one property per unit, did).
    let shapes_after = |objects: usize| {
        let source = format!(
            "function make() {{ var o = {{}}; for (var i = 0; i < 66000; i++) o['k' + i] = i; return o; }}
             var n = 0; for (var j = 0; j < {objects}; j++) n += make().k65999;"
        );
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&Engine::specialize(&source, "wide.js").unwrap()).unwrap();
        vm.shapes.len()
    };
    let growth = shapes_after(3) - shapes_after(1);
    assert!(growth < 16, "each object appended {growth} shapes");
}

#[test]
fn an_object_used_as_a_map_edits_its_own_dictionary_in_place() {
    // Deleting most keys gives the object a dictionary of its own: deletes edit it in place,
    // adds append past it, and none of them enters the transition cache.
    let shapes_after = |rounds: usize| {
        let source = format!(
            "function f(s) {{ const o = {{}}; let t = s; for (let i = 0; i < 1e4; i++) {{ o['d' + (i & 255)] = i; if (i & 1) delete o['d' + ((i * 7) & 255)]; }} for (const k in o) t += o[k]; return t; }}
             var n = 0; for (var j = 0; j < {rounds}; j++) n += f(j);"
        );
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&Engine::specialize(&source, "map.js").unwrap()).unwrap();
        vm.transitions.children().count()
    };
    let (one, three) = (shapes_after(1), shapes_after(3));
    assert!(three - one < 16, "two more rounds cached {} transitions", three - one);
    assert!(one < 3000, "one round cached {one} transitions");
}

#[test]
fn an_owned_dictionary_packs_its_storage_as_keys_come_and_go() {
    // At most 256 keys live at once, while 100000 come and go: its slots stay near the live count.
    let source = "var o = {}; for (var i = 0; i < 64; i++) o['k' + i] = i; for (var i = 0; i < 60; i++) delete o['k' + i];
        for (var i = 0; i < 1e5; i++) { o['x' + (i & 255)] = i; delete o['x' + ((i * 7) & 255)]; }";
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&Engine::specialize(source, "churn.js").unwrap()).unwrap();
    let owned = vm.shapes.iter().filter(|shape| shape.dictionary_trigger == Some(DictionaryTrigger::DeletionPattern));
    let widest = owned.map(Shape::storage_len).max().unwrap_or(0);
    assert!(widest <= 2 * 260, "an owned dictionary grew to {widest} slots");
}

#[test]
fn an_owned_dictionary_keeps_order_attributes_and_symbols() {
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let source = r#"
      var o = globalThis.o = {}, sym = Symbol("s");
      for (var i = 0; i < 40; i++) o["k" + i] = i;
      for (var i = 0; i < 30; i++) delete o["k" + i];
      o[sym] = "sv"; o.k3 = "back";
      Object.defineProperty(o, "ro", { value: 1, enumerable: false });
      Object.defineProperty(o, "acc", { get: function () { return "got"; }, enumerable: true, configurable: true });
      delete o.k35;
      sym = null;
      var keys = [];
      for (var k in o) keys.push(k);
      var d = Object.getOwnPropertyDescriptor(o, "ro");
      var syms = Object.getOwnPropertySymbols(o);
      Object.freeze(o);
      var junk = []; for (var g = 0; g < 400000; g++) junk = [g, { g: g }, "s" + g];
      print([keys.join(","), Object.keys(o).length, o.acc, o.ro, d.writable, d.enumerable,
             syms.length, o[syms[0]], o.k31, Object.isFrozen(o), Reflect.deleteProperty(o, "k32"), o.k32, Object.getOwnPropertySymbols(o).length].join(" "));
    "#;
    let program = Engine::specialize(source, "owned.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().join("\n"),
        "k30,k31,k32,k33,k34,k36,k37,k38,k39,k3,acc 11 got 1 false false 1 sv 31 true false 32 1"
    );
}
