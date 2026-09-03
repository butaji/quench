struct ForwardingConstructorFact;

const DIRECT_CONSTRUCTOR_SLOTS: usize = 64;

fn forward_value(
    source: &crate::facts::ForwardValueSource,
    function: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    use crate::facts::ForwardValueSource::*;
    match source {
        Receiver => Some(receiver.clone()),
        ReceiverProperty(property) => crate::execute::get_property_result(receiver, property).ok(),
        Argument(index) => arguments.get(usize::from(*index)).cloned(),
        Integer(value) => Some(crate::value::Value::Number(f64::from(*value))),
        Capture(slot) => Some(function.captures.get(*slot)),
    }
}

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
    if is_canonical_function_apply(&apply, &initializer) {
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

fn is_canonical_function_apply(
    apply: &crate::value::Value,
    initializer: &crate::value::Value,
) -> bool {
    match apply {
        crate::value::Value::Builtin(crate::ops::Builtin::FunctionApply) => true,
        crate::value::Value::BoundFunction(bound) => {
            matches!(
                bound.target,
                crate::value::Value::Builtin(crate::ops::Builtin::FunctionApply)
            ) && bound.arguments.is_empty()
                && crate::builtins::same_value(Some(&bound.receiver), Some(initializer))
        }
        _ => false,
    }
}

fn direct_forward_receiver(
    initializer: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    let crate::value::Value::Function(function) = initializer else {
        crate::execution_trace::kernel("direct_forward_reject_initializer", true);
        return None;
    };
    let crate::value::Value::Object(receiver) = receiver else {
        crate::execution_trace::kernel("direct_forward_reject_receiver", true);
        return None;
    };
    if receiver.has_replacement() || receiver.hot_properties().len() != 1 {
        crate::execution_trace::kernel("direct_forward_reject_state", true);
        return None;
    }
    let prototype = receiver.hot_properties().slot_value(0)?;
    let value = direct_constructor_object(function, prototype, arguments);
    if value.is_none() {
        crate::execution_trace::kernel("direct_forward_reject_plan", true);
    }
    value
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

pub(crate) fn composed_constructor_object(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    prototype: crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(prototype_object) = &prototype else { return None };
    (!prototype_object.has_replacement()).then_some(())?;
    let mut properties = crate::value::ObjectProperties::with_capacity(10);
    properties.push(("\0prototype".into(), prototype.clone()));
    apply_constructor_steps(function, arguments, &mut properties, 0)?;
    validate_composed_fields(&prototype, &properties)?;
    let value = crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::from_shared_properties(properties),
    ));
    Some(value)
}

fn validate_composed_fields(
    prototype: &crate::value::Value,
    properties: &crate::value::ObjectProperties,
) -> Option<()> {
    for name in properties.names().filter(|name| name.as_str() != "\0prototype") {
        crate::property_define::accessor(prototype, name, "set").is_none().then_some(())?;
        (!crate::properties::inherited_write_blocked(prototype, name)).then_some(())?;
    }
    Some(())
}

fn apply_constructor_steps(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    arguments: &[crate::value::Value],
    properties: &mut crate::value::ObjectProperties,
    depth: usize,
) -> Option<()> {
    (depth < 8).then_some(())?;
    let steps = function.code.facts().composed_constructor.as_ref();
    for step in steps {
        match step {
            crate::facts::ComposedConstructorStep::Field(field) => {
                let value = direct_constructor_value(
                    &field.source, function, arguments, &std::cell::Cell::new(0),
                )?;
                store_constructor_field(properties, field.name.as_str(), value);
            }
            crate::facts::ComposedConstructorStep::SuperCall { owner_slot, arguments: sources } => {
                let (super_function, forwarded) = composed_super_target(
                    function, *owner_slot, sources, arguments,
                )?;
                apply_constructor_body(&super_function, &forwarded, properties, depth + 1)?;
            }
        }
    }
    Some(())
}

fn apply_constructor_body(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    arguments: &[crate::value::Value],
    properties: &mut crate::value::ObjectProperties,
    depth: usize,
) -> Option<()> {
    if !function.code.facts().composed_constructor.is_empty() {
        return apply_constructor_steps(function, arguments, properties, depth);
    }
    let cache = std::cell::Cell::new(0);
    for field in function.code.facts().direct_constructor.iter() {
        let value = direct_constructor_value(&field.source, function, arguments, &cache)?;
        store_constructor_field(properties, field.name.as_str(), value);
    }
    (!function.code.facts().direct_constructor.is_empty()).then_some(())
}

fn composed_super_target(
    function: &crate::value::FunctionValue,
    owner_slot: u16,
    sources: &[crate::facts::ForwardValueSource],
    arguments: &[crate::value::Value],
) -> Option<(std::rc::Rc<crate::value::FunctionValue>, Vec<crate::value::Value>)> {
    let crate::value::Value::Function(owner) = crate::locals::resolved_replacement(
        function.captures.get(owner_slot),
    ) else { return None };
    let target = crate::locals::resolved_replacement(
        crate::vm::proven_function_own_data(&owner, "superConstructor")?,
    );
    let crate::value::Value::Function(target) = crate::construct::peel_construct_value(&target)
        else { return None };
    let forwarded = sources.iter().map(|source| forward_value(
        source, function, &crate::value::Value::Undefined, arguments,
    )).collect::<Option<Vec<_>>>()?;
    Some((target, forwarded))
}

fn store_constructor_field(
    properties: &mut crate::value::ObjectProperties,
    name: &str,
    value: crate::value::Value,
) {
    if let Some(slot) = properties.position_rev(name) {
        properties.store_slot(slot, value);
    } else {
        properties.push((name.into(), value));
    }
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
        crate::facts::DirectConstructorSource::EmptyArray => {
            guarded_array(function, global_array_cache, &[])
        }
        crate::facts::DirectConstructorSource::FalsyArgumentOrInteger { argument, fallback } => {
            let value = arguments.get(usize::from(*argument)).cloned()
                .unwrap_or(crate::value::Value::Undefined);
            Some(if crate::conversion::to_boolean(&value) { value }
                else { crate::value::Value::Number(f64::from(*fallback)) })
        }
        crate::facts::DirectConstructorSource::ConstructCapture {
            constructor_slot, arguments: sources,
        } => {
            let crate::value::Value::Function(constructor) = crate::locals::resolved_replacement(
                function.captures.get(*constructor_slot),
            ) else { return None };
            let nested = sources.iter().map(|source| forward_value(
                source, function, &crate::value::Value::Undefined, arguments,
            )).collect::<Option<Vec<_>>>()?;
            let prototype = crate::locals::resolved_replacement(
                crate::vm::proven_function_own_data(&constructor, "prototype")?,
            );
            direct_constructor_object(&constructor, prototype, &nested)
        }
        crate::facts::DirectConstructorSource::CaptureProperty { owner_slot, property } => {
            let crate::value::Value::Function(owner) = crate::locals::resolved_replacement(
                function.captures.get(*owner_slot),
            ) else { return None };
            Some(crate::locals::resolved_replacement(
                crate::vm::proven_function_own_data(&owner, property)?,
            ))
        }
        crate::facts::DirectConstructorSource::GuardedArray { length_slot } => {
            guarded_array(function, global_array_cache, &[function.captures.get(*length_slot)])
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

fn guarded_array(
    function: &crate::value::FunctionValue,
    cache: &std::cell::Cell<u64>,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    let constructor = crate::vm::get_named_property_result(
        &function.captures.get(0), "Array", cache,
    ).ok()?;
    matches!(constructor, crate::value::Value::Builtin(crate::ops::Builtin::Array))
        .then_some(())?;
    let value = crate::builtins::array(arguments).ok()?;
    Some(value)
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
    if fields.is_empty() {
        return None;
    }
    direct_constructor_prototype(prototype, &fields)?;
    let fields: std::rc::Rc<[_]> = fields.into();
    Some(install_direct_constructor(
        slot, function, prototype, fields,
    ))
}

pub(crate) fn direct_constructor_allocation_is_scalarizable(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    prototype: &crate::value::Value,
) -> bool {
    function.instance_fields.borrow().is_empty()
        && crate::functions::is_constructible(function)
        && direct_constructor_plan(function, prototype).is_some()
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
        crate::property_define::accessor(&prototype, field, "set")
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
