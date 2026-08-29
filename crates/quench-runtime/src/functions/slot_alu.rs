fn execute_slot_alu(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    let crate::facts::DirectMethodFact::SlotDot3 {
        receiver: receiver_slots,
        argument: argument_slots,
    } = function.code.facts().direct_method.as_deref()?
    else {
        return None;
    };
    let [argument] = arguments else {
        return None;
    };
    let crate::value::Value::Object(receiver) = receiver else {
        return None;
    };
    let crate::value::Value::Object(argument) = argument else {
        return None;
    };
    let product = |index: usize| {
        let left = crate::vm::proven_own_word(receiver, &receiver_slots[index])?.number()?;
        let right = crate::vm::proven_own_word(argument, &argument_slots[index])?.number()?;
        Some(left * right)
    };
    // Preserve the source tree's left-associated IEEE-754 additions exactly.
    let result = (product(0)? + product(1)?) + product(2)?;
    crate::execution_trace::kernel("S|P", false);
    Some(crate::value::Value::Number(result))
}
