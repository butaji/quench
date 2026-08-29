struct ForwardingConstructorFact;

const DIRECT_CONSTRUCTOR_SLOTS: usize = 64;

struct DirectConstructorPlan {
    function: std::rc::Weak<crate::value::FunctionValue>,
    prototype: std::rc::Weak<crate::value::ObjectData>,
    prototype_layout: u32,
    intrinsic_generation: u64,
    fields: std::rc::Rc<
        [(
            crate::value::PropertyName,
            crate::facts::DirectConstructorSource,
        )],
    >,
    global_array_cache: std::cell::Cell<u64>,
}

thread_local! {
    static DIRECT_CONSTRUCTORS: std::cell::RefCell<Vec<Option<std::rc::Rc<DirectConstructorPlan>>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl ForwardingConstructorFact {
    fn recognize(function: &crate::value::FunctionValue) -> Option<Self> {
        use crate::ir::Opcode::*;
        const SHAPE: [crate::ir::Opcode; 8] = [
            LoadLocalChecked,
            GetN,
            GetN,
            LoadLocalChecked,
            LoadLocal,
            CallN,
            LoadConst,
            Return,
        ];
        (function.params == 0 && matches!(function.kind, crate::ops::FunctionKind::Ordinary))
            .then_some(())?;
        let code = function.code.code()?;
        (code.len() == SHAPE.len()).then_some(())?;
        SHAPE
            .into_iter()
            .enumerate()
            .all(|(pc, opcode)| code.instruction(pc).is_some_and(|op| op.opcode == opcode))
            .then_some(())?;
        validate_forwarding_operands(function, code)?;
        Some(Self)
    }
}

fn validate_forwarding_operands(
    function: &crate::value::FunctionValue,
    code: crate::machine::CodeView<'_>,
) -> Option<()> {
    let arguments = function.captures.len() as u16;
    let this = arguments.checked_add(1)?;
    let [load_this, initialize, apply, call_this, call_arguments] =
        [0, 1, 2, 3, 4].map(|pc| code.instruction(pc).unwrap());
    (load_this.b == this && initialize.b == load_this.a).then_some(())?;
    (apply.b == initialize.a && call_this.b == this && call_arguments.b == arguments)
        .then_some(())?;
    (code.metadata_at(1)?.name.as_deref() == Some("initialize")
        && code.metadata_at(2)?.name.as_deref() == Some("apply"))
    .then_some(())?;
    validate_forwarding_call(code, apply.a, call_this.a, call_arguments.a)
}

fn validate_forwarding_call(
    code: crate::machine::CodeView<'_>,
    callee: u16,
    this: u16,
    arguments: u16,
) -> Option<()> {
    let call = code.instruction(5)?;
    let first_argument = call.a.checked_sub(u16::from(call.flags))?;
    (call.opcode == crate::ir::Opcode::CallN
        && call.flags == 2
        && call.b == code.instruction(1)?.a
        && call.c == callee
        && first_argument == this
        && first_argument.checked_add(1)? == arguments)
        .then_some(())
}

fn execute_forwarding_constructor(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<(crate::value::Value, crate::value::Value), crate::execute::VmError>> {
    ForwardingConstructorFact::recognize(function)?;
    Some(forward_constructor(function, this_value, arguments))
}

fn forward_constructor(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let initializer = crate::execute::get_property_result(this_value, "initialize")?;
    let apply = crate::execute::get_property_result(&initializer, "apply")?;
    if matches!(
        apply,
        crate::value::Value::Builtin(crate::ops::Builtin::FunctionApply)
    ) {
        if let Some(receiver) = direct_forward_receiver(&initializer, this_value, arguments) {
            crate::execution_trace::kernel("direct_forward_constructor", false);
            return Ok((crate::value::Value::Undefined, receiver));
        }
        crate::functions::execute_target(&initializer, this_value, arguments)?;
    } else {
        forward_custom_apply(function, &initializer, &apply, this_value, arguments)?;
    }
    crate::execution_trace::kernel("forwarding_constructor", false);
    let final_this = crate::locals::resolved_replacement(this_value.clone());
    Ok((crate::value::Value::Undefined, final_this))
}

fn direct_forward_receiver(
    initializer: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    let crate::value::Value::Function(function) = initializer else {
        return None;
    };
    let crate::value::Value::Object(receiver) = receiver else {
        return None;
    };
    if receiver.has_replacement() || receiver.hot_properties().len() != 1 {
        return None;
    }
    let prototype = receiver.hot_properties().slot_value(0)?;
    direct_constructor_object(function, prototype, arguments)
}

pub(crate) fn direct_constructor_object(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    prototype: crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    let plan = direct_constructor_plan(function, &prototype)?;
    let mut properties = crate::value::ObjectProperties::with_capacity(plan.fields.len() + 1);
    properties.push(("\0prototype".into(), prototype));
    for (name, source) in plan.fields.iter() {
        properties.push((
            name.clone(),
            direct_constructor_value(source, function, arguments, &plan.global_array_cache)?,
        ));
    }
    Some(crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::from_shared_properties(properties),
    )))
}

fn direct_constructor_value(
    source: &crate::facts::DirectConstructorSource,
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
    global_array_cache: &std::cell::Cell<u64>,
) -> Option<crate::value::Value> {
    match source {
        crate::facts::DirectConstructorSource::Argument(source) => Some(
            arguments
                .get(usize::from(*source))
                .cloned()
                .unwrap_or(crate::value::Value::Undefined),
        ),
        crate::facts::DirectConstructorSource::Boolean(value) => {
            Some(crate::value::Value::Boolean(*value))
        }
        crate::facts::DirectConstructorSource::Integer(value) => {
            Some(crate::value::Value::Number(f64::from(*value)))
        }
        crate::facts::DirectConstructorSource::Null => Some(crate::value::Value::Null),
        crate::facts::DirectConstructorSource::GuardedArray { length_slot } => {
            let global = function.captures.get(0);
            let constructor =
                crate::vm::get_named_property_result(&global, "Array", global_array_cache)
                    .ok()?;
            if !matches!(
                constructor,
                crate::value::Value::Builtin(crate::ops::Builtin::Array)
            ) {
                return None;
            }
            crate::builtins::array(&[function.captures.get(*length_slot)]).ok()
        }
        crate::facts::DirectConstructorSource::NullishSelectCapture {
            argument,
            nullish_slot,
            other_slot,
        } => {
            let slot = if arguments
                .get(usize::from(*argument))
                .unwrap_or(&crate::value::Value::Undefined)
                .is_nullish()
            {
                *nullish_slot
            } else {
                *other_slot
            };
            Some(function.captures.get(slot))
        }
    }
}

fn direct_constructor_plan(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    prototype: &crate::value::Value,
) -> Option<std::rc::Rc<DirectConstructorPlan>> {
    let crate::value::Value::Object(prototype) = prototype else {
        return None;
    };
    let slot = (std::rc::Rc::as_ptr(function) as usize >> 4) & (DIRECT_CONSTRUCTOR_SLOTS - 1);
    if let Some(plan) = cached_direct_constructor(slot, function, prototype) {
        return Some(plan);
    }
    let fields = direct_constructor_fields(function);
    if fields.len() < 3 {
        return None;
    }
    direct_constructor_prototype(prototype, &fields)?;
    let fields: std::rc::Rc<[_]> = fields.into();
    Some(install_direct_constructor(
        slot, function, prototype, fields,
    ))
}

fn direct_constructor_fields(
    function: &crate::value::FunctionValue,
) -> Vec<(
    crate::value::PropertyName,
    crate::facts::DirectConstructorSource,
)> {
    function
        .code
        .facts()
        .direct_constructor
        .iter()
        .map(|field| (field.name.clone().into(), field.source.clone()))
        .collect()
}

fn direct_constructor_prototype(
    prototype: &std::rc::Rc<crate::value::ObjectData>,
    fields: &[(
        crate::value::PropertyName,
        crate::facts::DirectConstructorSource,
    )],
) -> Option<()> {
    if prototype.has_replacement() || !direct_constructor_plain_parent(prototype) {
        return None;
    }
    let prototype = crate::value::Value::Object(prototype.clone());
    for (field, _) in fields {
        crate::properties::define::accessor(&prototype, field, "set")
            .is_none()
            .then_some(())?;
        (!crate::properties::inherited_write_blocked(&prototype, field)).then_some(())?;
    }
    Some(())
}

fn direct_constructor_plain_parent(prototype: &crate::value::ObjectData) -> bool {
    let parent = prototype
        .hot_properties()
        .position_rev("\0prototype")
        .and_then(|slot| prototype.hot_properties().slot_value(slot));
    parent.is_none_or(|value| {
        matches!(
            value,
            crate::value::Value::Builtin(crate::ops::Builtin::ObjectPrototype)
        )
    })
}

fn cached_direct_constructor(
    slot: usize,
    function: &std::rc::Rc<crate::value::FunctionValue>,
    prototype: &std::rc::Rc<crate::value::ObjectData>,
) -> Option<std::rc::Rc<DirectConstructorPlan>> {
    DIRECT_CONSTRUCTORS.with(|plans| {
        let plans = plans.borrow();
        let plan = plans.get(slot)?.as_ref()?;
        let stored_function = plan.function.upgrade()?;
        let stored_prototype = plan.prototype.upgrade()?;
        (std::rc::Rc::ptr_eq(&stored_function, function)
            && std::rc::Rc::ptr_eq(&stored_prototype, prototype)
            && !prototype.has_replacement()
            && prototype.semantic_layout_id() == plan.prototype_layout
            && crate::builtins::intrinsic_override_generation() == plan.intrinsic_generation)
            .then(|| std::rc::Rc::clone(plan))
    })
}

fn install_direct_constructor(
    slot: usize,
    function: &std::rc::Rc<crate::value::FunctionValue>,
    prototype: &std::rc::Rc<crate::value::ObjectData>,
    fields: std::rc::Rc<
        [(
            crate::value::PropertyName,
            crate::facts::DirectConstructorSource,
        )],
    >,
) -> std::rc::Rc<DirectConstructorPlan> {
    let plan = std::rc::Rc::new(DirectConstructorPlan {
        function: std::rc::Rc::downgrade(function),
        prototype: std::rc::Rc::downgrade(prototype),
        prototype_layout: prototype.semantic_layout_id(),
        intrinsic_generation: crate::builtins::intrinsic_override_generation(),
        fields,
        global_array_cache: std::cell::Cell::new(0),
    });
    DIRECT_CONSTRUCTORS.with(|plans| {
        let mut plans = plans.borrow_mut();
        if plans.is_empty() {
            plans.resize_with(DIRECT_CONSTRUCTOR_SLOTS, || None);
        }
        plans[slot] = Some(std::rc::Rc::clone(&plan));
    });
    plan
}

fn forward_custom_apply(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    initializer: &crate::value::Value,
    apply: &crate::value::Value,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(), crate::execute::VmError> {
    let environment = crate::environment::Environment::new();
    let list = arguments_object(function, arguments.to_vec(), &environment);
    crate::functions::execute_target(apply, initializer, &[this_value.clone(), list])?;
    Ok(())
}
