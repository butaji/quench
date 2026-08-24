#[derive(Clone, Copy, PartialEq)]
enum RecordOrigin {
    Unknown,
    This,
    Argument(u16),
    Undefined,
}

#[derive(Clone, Copy)]
struct RecordConstructorFact<'a> {
    fields: [(&'a str, u16); 2],
}

impl<'a> RecordConstructorFact<'a> {
    fn recognize(function: &'a crate::value::FunctionValue) -> Option<Self> {
        use crate::ir::Opcode;
        let code = function.code.code()?;
        let mut registers = [(u16::MAX, RecordOrigin::Unknown); 16];
        let mut used = 0;
        let mut fields = [("", 0); 2];
        let mut field_count = 0;
        for pc in 0..code.len() {
            let instruction = code.instruction(pc)?;
            match instruction.opcode {
                Opcode::LoadLocalChecked => set_record_origin(
                    &mut registers,
                    &mut used,
                    instruction.a,
                    local_record_origin(function, instruction.b)?,
                )?,
                Opcode::Move => {
                    let origin = record_origin(&registers, used, instruction.b);
                    set_record_origin(&mut registers, &mut used, instruction.a, origin)?;
                }
                Opcode::LoadConst => {
                    let (_, crate::ops::Constant::Undefined) = code.constant_at(pc)? else {
                        return None;
                    };
                    set_record_origin(
                        &mut registers,
                        &mut used,
                        instruction.a,
                        RecordOrigin::Undefined,
                    )?;
                }
                Opcode::SetN => {
                    (record_origin(&registers, used, instruction.a) == RecordOrigin::This)
                        .then_some(())?;
                    let RecordOrigin::Argument(argument) =
                        record_origin(&registers, used, instruction.b)
                    else {
                        return None;
                    };
                    let field = fields.get_mut(field_count)?;
                    *field = (code.metadata_at(pc)?.name.as_deref()?, argument);
                    field_count += 1;
                }
                Opcode::Return => {
                    (record_origin(&registers, used, instruction.a) == RecordOrigin::Undefined)
                        .then_some(())?;
                }
                _ => return None,
            }
        }
        (field_count == 2 && fields[0].0 != fields[1].0).then_some(Self { fields })
    }
}

fn local_record_origin(function: &crate::value::FunctionValue, slot: u16) -> Option<RecordOrigin> {
    let captures = u16::try_from(function.captures.len()).ok()?;
    if slot < captures {
        return None;
    }
    let relative = slot - captures;
    if relative < function.params {
        return Some(RecordOrigin::Argument(relative));
    }
    (relative == function.params + 1).then_some(RecordOrigin::This)
}

fn record_origin(entries: &[(u16, RecordOrigin); 16], used: usize, register: u16) -> RecordOrigin {
    entries[..used]
        .iter()
        .rev()
        .find_map(|(id, value)| (*id == register).then_some(*value))
        .unwrap_or(RecordOrigin::Unknown)
}

fn set_record_origin(
    entries: &mut [(u16, RecordOrigin); 16],
    used: &mut usize,
    register: u16,
    value: RecordOrigin,
) -> Option<()> {
    if let Some(entry) = entries[..*used].iter_mut().find(|(id, _)| *id == register) {
        entry.1 = value;
        return Some(());
    }
    let entry = entries.get_mut(*used)?;
    *entry = (register, value);
    *used += 1;
    Some(())
}

fn try_record_constructor(
    function: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    let fact = RecordConstructorFact::recognize(function)?;
    record_prototype_is_data_only(receiver, &fact)?;
    let crate::value::Value::Object(receiver) = receiver else {
        return None;
    };
    let prototype = receiver
        .hot_properties()
        .iter()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))?;
    let mut properties = Vec::with_capacity(3);
    properties.push(("\0prototype".to_string(), prototype));
    for (key, argument) in fact.fields {
        let value = arguments
            .get(usize::from(argument))
            .cloned()
            .unwrap_or(crate::value::Value::Undefined);
        properties.push((
            key.to_string(),
            crate::value::Value::BindingCell(std::rc::Rc::new(std::cell::RefCell::new(value))),
        ));
    }
    Some(crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}

fn record_prototype_is_data_only(
    receiver: &crate::value::Value,
    fact: &RecordConstructorFact<'_>,
) -> Option<()> {
    let crate::value::Value::Object(receiver) = receiver else {
        return None;
    };
    let prototype = receiver
        .hot_properties()
        .iter()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))?;
    let crate::value::Value::Object(prototype) = prototype else {
        return None;
    };
    if prototype.has_replacement() {
        return None;
    }
    for (key, _) in fact.fields {
        let conflicts = prototype.hot_properties().iter().any(|(name, _)| {
            name == key
                || crate::builtins::is_descriptor_key_for(name, key)
                || crate::builtins::is_deleted_key_for(name, key)
        });
        if conflicts {
            return None;
        }
    }
    let parent = prototype
        .hot_properties()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value));
    if parent.is_some_and(|value| {
        !matches!(
            value,
            crate::value::Value::Builtin(crate::ops::Builtin::ObjectPrototype)
        )
    }) {
        return None;
    }
    for (key, _) in fact.fields {
        if crate::builtins::read_intrinsic_override(crate::ops::Builtin::ObjectPrototype, key)
            .is_some()
            || crate::builtins::object::builtin_owns_property(
                crate::ops::Builtin::ObjectPrototype,
                key,
            )
        {
            return None;
        }
    }
    Some(())
}
