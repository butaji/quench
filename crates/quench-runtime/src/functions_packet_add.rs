const PACKET_ADD_FACT_SLOTS: usize = 64;

struct PacketAddFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    admitted: bool,
}

thread_local! {
    static PACKET_ADD_FACTS: std::cell::RefCell<Vec<Option<PacketAddFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline]
fn is_packet_add_candidate(function: &crate::value::FunctionValue) -> bool {
    function.params == 1
        && function.code.capture_slots().len() == 4
        && function.code.code().is_some_and(|code| code.len() == 23)
}

fn execute_packet_add(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    if !packet_add_fact(function) {
        return None;
    }
    let crate::value::Value::Object(packet) = receiver else {
        return None;
    };
    if packet.has_replacement() {
        return None;
    }
    let packet_link = writable_own_word(packet, "link")?;
    let queue = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    if queue.is_nullish() {
        packet_link.store(crate::value::Value::Null);
        crate::execution_trace::kernel("packet_add_word_slots", false);
        return Some(receiver.clone());
    }
    let crate::value::Value::Object(head) = &queue else {
        return None;
    };
    let packet_ptr = std::rc::Rc::as_ptr(packet);
    let head_ptr = std::rc::Rc::as_ptr(head);
    let tail_link = packet_tail_link(head_ptr, packet_ptr)?;

    packet_link.store(crate::value::Value::Null);
    // SAFETY: admission retained `queue`, proved every traversed object current,
    // and performed no call or shape mutation between validation and this store.
    unsafe { &*tail_link }.store(receiver.clone());
    crate::execution_trace::kernel("packet_add_word_slots", false);
    Some(queue)
}

fn packet_tail_link(
    head: *const crate::value::ObjectData,
    packet: *const crate::value::ObjectData,
) -> Option<*const crate::register_file::SlotWord> {
    if head == packet {
        return None;
    }
    let mut slow = Some(head);
    let mut fast = Some(head);
    loop {
        slow = advance_optional(slow, packet)?;
        fast = advance_optional(advance_optional(fast, packet)?, packet)?;
        match (slow, fast) {
            (Some(left), Some(right)) if left == right => return None,
            (None, _) | (_, None) => break,
            _ => {}
        }
    }
    let mut tail = head;
    loop {
        match advance_packet(tail, packet)? {
            Some(next) => tail = next,
            None => return packet_link_word(tail, packet),
        }
    }
}

fn advance_optional(
    object: Option<*const crate::value::ObjectData>,
    packet: *const crate::value::ObjectData,
) -> Option<Option<*const crate::value::ObjectData>> {
    object.map_or(Some(None), |object| advance_packet(object, packet))
}

fn advance_packet(
    object: *const crate::value::ObjectData,
    packet: *const crate::value::ObjectData,
) -> Option<Option<*const crate::value::ObjectData>> {
    let word = packet_link_word(object, packet)?;
    // SAFETY: `packet_link_word` returns a slot in a strongly owned, current
    // object and validation performs no mutation that can move its storage.
    unsafe { &*word }.object_or_null_ptr()
}

fn packet_link_word(
    object: *const crate::value::ObjectData,
    packet: *const crate::value::ObjectData,
) -> Option<*const crate::register_file::SlotWord> {
    if object == packet {
        return None;
    }
    // SAFETY: the head and each successor remain strongly owned by `queue` or
    // its predecessor's execute word throughout validation.
    let object = unsafe { &*object };
    if object.has_replacement() {
        return None;
    }
    writable_own_word(object, "link").map(std::ptr::from_ref)
}

fn packet_add_fact(function: &std::rc::Rc<crate::value::FunctionValue>) -> bool {
    let pointer = std::rc::Rc::as_ptr(function);
    let index = (pointer as usize >> 4) & (PACKET_ADD_FACT_SLOTS - 1);
    if let Some(admitted) = PACKET_ADD_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.admitted)
    }) {
        return admitted;
    }
    let admitted = match_packet_add(function);
    PACKET_ADD_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(PACKET_ADD_FACT_SLOTS, || None);
        }
        facts[index] = Some(PacketAddFact {
            function: std::rc::Rc::downgrade(function),
            admitted,
        });
    });
    admitted
}

fn match_packet_add(function: &crate::value::FunctionValue) -> bool {
    let Some(code) = function.code.code() else {
        return false;
    };
    if function.params != 1 || code.len() != 23 {
        return false;
    }
    let Some(crate::ops::Op::Loop { test, body, .. }) = code.cold_at(13) else {
        return false;
    };
    let Some(crate::ops::Op::Branch {
        then_ops, else_ops, ..
    }) = code.cold_at(9)
    else {
        return false;
    };
    packet_add_main_shape(code)
        && packet_add_test_shape(test.code())
        && packet_add_body_shape(body.code())
        && packet_add_return_shape(then_ops.code(), else_ops.code())
}

fn packet_add_main_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    let ops: [_; 23] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (Move, ops[0].a)
        && (ops[2].opcode, ops[2].b) == (Move, ops[1].a)
        && matches!(code.constant_at(3), Some((_, crate::ops::Constant::Null)))
        && (ops[4].opcode, ops[4].a, ops[4].b) == (SetN, ops[2].a, ops[3].a)
        && is_local_load(ops[5])
        && matches!(code.constant_at(6), Some((_, crate::ops::Constant::Null)))
        && code.binary_at(7).is_some_and(|(_, op, left, right)| {
            op == crate::ops::BinaryOp::Equal && (left, right) == (ops[5].a, ops[6].a)
        })
        && ops[9].opcode == Slow
        && is_local_load(ops[10])
        && (ops[11].opcode, ops[11].b) == (InitLocal, ops[10].a)
        && ops[13].opcode == Slow
        && is_local_load(ops[14])
        && (ops[15].opcode, ops[15].b) == (Move, ops[14].a)
        && (ops[16].opcode, ops[16].b) == (Move, ops[15].a)
        && is_local_load(ops[17])
        && (ops[18].opcode, ops[18].a, ops[18].b) == (SetN, ops[16].a, ops[17].a)
        && is_local_load(ops[19])
        && (ops[20].opcode, ops[20].a) == (Return, ops[19].a)
        && code.metadata_at(4).and_then(|meta| meta.name.as_deref()) == Some("link")
        && code.metadata_at(18).and_then(|meta| meta.name.as_deref()) == Some("link")
}

fn packet_add_test_shape(code: Option<crate::machine::CodeView<'_>>) -> bool {
    let Some(code) = code.filter(|code| code.len() == 6) else {
        return false;
    };
    use crate::ir::Opcode::*;
    let ops: [_; 6] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (GetN, ops[0].a)
        && (ops[2].opcode, ops[2].b) == (StoreLocal, ops[1].a)
        && matches!(code.constant_at(3), Some((_, crate::ops::Constant::Null)))
        && code.binary_at(4).is_some_and(|(_, op, left, right)| {
            op == crate::ops::BinaryOp::NotEqual && (left, right) == (ops[1].a, ops[3].a)
        })
        && (ops[5].opcode, ops[5].a) == (Return, ops[4].a)
        && code.metadata_at(1).and_then(|meta| meta.name.as_deref()) == Some("link")
}

fn packet_add_body_shape(code: Option<crate::machine::CodeView<'_>>) -> bool {
    let Some(code) = code.filter(|code| code.len() == 1) else {
        return false;
    };
    code.instruction(0)
        .is_some_and(|op| op.opcode == crate::ir::Opcode::Move && op.flags == 1)
}

fn packet_add_return_shape(
    then_code: Option<crate::machine::CodeView<'_>>,
    else_code: Option<crate::machine::CodeView<'_>>,
) -> bool {
    let (Some(then_code), Some(else_code)) = (then_code, else_code) else {
        return false;
    };
    then_code.len() == 3
        && else_code.is_empty()
        && then_code.instruction(0).is_some_and(is_local_load)
        && then_code
            .instruction(1)
            .is_some_and(|op| op.opcode == crate::ir::Opcode::Return)
}
