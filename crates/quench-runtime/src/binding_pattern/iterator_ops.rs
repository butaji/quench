fn iterator_start(source: u16, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let iterator = take_register(next);
    ops.push(Op::GetIterator {
        dst: iterator,
        iterable: source,
    });
    iterator
}

fn iterator_step(iterator: u16, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let value = take_register(next);
    ops.push(Op::IteratorStep {
        dst: value,
        iterator,
    });
    value
}

fn iterator_rest(iterator: u16, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let value = take_register(next);
    ops.push(Op::IteratorRest {
        dst: value,
        iterator,
    });
    value
}
