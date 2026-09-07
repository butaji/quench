use super::super::*;

fn get_named(receiver: Register) -> Instruction {
    Instruction {
        opcode: Opcode::GetN,
        flags: 0,
        a: 5,
        b: receiver,
        c: 0,
    }
}

#[test]
fn block_value_graph_selects_local_property_through_aliases() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_local(2, 20), |_| None));
    assert!(graph.push(Instruction::move_(3, 2), |_| None));
    assert!(graph.push(Instruction::load_local(4, 21), |_| None));
    let selected = graph
        .select_property(get_named(3), &BTreeSet::new())
        .unwrap();
    assert_eq!(selected.receiver_slot, 20);
    assert_eq!(selected.result.register, 5);
    assert_eq!(selected.span, 4);
    assert_eq!(selected.discarded.iter().flatten().count(), 3);
}

#[test]
fn block_value_graph_rejects_unsafe_property_inputs() {
    let get = get_named(2);
    let mut local_graph = BlockValueGraph::new();
    assert!(local_graph.push(Instruction::load_local(2, 20), |_| None));
    assert!(local_graph
        .select_property(get, &BTreeSet::from([2]))
        .is_none());
    assert!(local_graph
        .select_property(Instruction { flags: 1, ..get }, &BTreeSet::new())
        .is_none());

    let mut constant_graph = BlockValueGraph::new();
    assert!(constant_graph.push(Instruction::load_const(2, 7), |_| Some(1.0_f64.to_bits())));
    assert!(constant_graph
        .select_property(get, &BTreeSet::new())
        .is_none());
}
