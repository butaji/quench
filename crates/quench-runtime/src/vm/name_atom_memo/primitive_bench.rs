//! Actual atom queries with controlled memo placement; setup is outside timing.

extern crate test;

use super::{NAME_ATOM_MEMO_ENTRIES, NameAtomMemo};
use std::hint::black_box;
use test::Bencher;

const BATCH_QUERIES: usize = 64;

fn query_pair<const COLLIDING: bool, const HASH_LOOKUP: bool>(b: &mut Bencher) {
    // More names than slots guarantees a collision. Keep their allocations alive:
    // cloning the selected strings would change the addresses being measured.
    let names: Vec<_> = (0..=NAME_ATOM_MEMO_ENTRIES)
        .map(|index| format!("primitive-name-{index:08}"))
        .collect();
    let mut slots = [None; NAME_ATOM_MEMO_ENTRIES];
    let mut pair = None;
    for (index, name) in names.iter().enumerate() {
        let slot = NameAtomMemo::index(name);
        let previous = if COLLIDING {
            slots[slot]
        } else {
            slots.iter().enumerate().find_map(
                |(other, previous)| {
                    if other != slot { *previous } else { None }
                },
            )
        };
        if let Some(previous) = previous {
            pair = Some([previous, index]);
            break;
        }
        slots[slot] = Some(index);
    }
    let pair = pair.expect("fixture must provide the requested memo placement");
    let inputs = pair.map(|index| names[index].as_str());
    eprintln!(
        "name query: colliding={COLLIDING}, hash_lookup={HASH_LOOKUP}, names={inputs:?}, slots={:?}",
        inputs.map(NameAtomMemo::index)
    );
    assert_eq!(
        NameAtomMemo::index(inputs[0]) == NameAtomMemo::index(inputs[1]),
        COLLIDING
    );
    let program = crate::Engine::specialize("", "primitive-name-query.js").unwrap();
    let mut vm = super::super::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let expected = inputs.map(|name| vm.intern_atom(name));
    assert_ne!(expected[0], expected[1]);
    assert_eq!(vm.name_atom_memo.candidate(inputs[0]).is_some(), !COLLIDING);
    assert_eq!(vm.name_atom_memo.candidate(inputs[1]), Some(expected[1]));
    let query = |vm: &mut super::super::Vm<crate::test_host::TestHost>, name| {
        if HASH_LOOKUP {
            vm.lookup_atom(name).unwrap()
        } else {
            vm.intern_atom(name)
        }
    };
    for (name, expected) in inputs.into_iter().zip(expected) {
        assert_eq!(query(&mut vm, name), expected);
    }
    b.iter(|| {
        for index in 0..BATCH_QUERIES {
            black_box(query(
                black_box(&mut vm),
                black_box(inputs[index % inputs.len()]),
            ));
        }
    });
    for (name, expected) in inputs.into_iter().zip(expected) {
        assert_eq!(query(&mut vm, name), expected);
    }
}

#[bench]
fn primitive_name_atom_warm_pair(b: &mut Bencher) {
    query_pair::<false, false>(b);
}

#[bench]
fn primitive_name_atom_colliding_pair(b: &mut Bencher) {
    query_pair::<true, false>(b);
}

#[bench]
fn primitive_name_atom_hash_lookup_pair(b: &mut Bencher) {
    query_pair::<false, true>(b);
}
