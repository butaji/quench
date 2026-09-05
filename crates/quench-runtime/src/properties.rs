use crate::{facts::ProgramDb, literal::reduce_literal, ops::Op};
use oxc::ast::ast::Expression;
use std::collections::HashMap;
const NON_EXTENSIBLE: &str = "\0quench:non_extensible";
include!("properties_optional.rs");
include!("properties_named_transition.rs");
pub(crate) fn reduce(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (object, key, optional) = match expression {
        Expression::StaticMemberExpression(member) => (
            &member.object,
            member.property.name.to_string(),
            member.optional,
        ),
        Expression::PrivateFieldExpression(member) => {
            return reduce_private_get(member, ops, facts, next_register, locals);
        }
        Expression::ComputedMemberExpression(member) => {
            return reduce_computed_get(member, ops, facts, next_register, locals);
        }
        _ => return None,
    };
    emit_get(object, key, optional, ops, facts, next_register, locals)
}

fn reduce_computed_get(
    member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(member.object, Expression::Super(_)) {
        ops.push(Op::CheckSuperThis);
        let key = crate::reduce::reduce_expression(
            &member.expression,
            ops,
            facts,
            next_register,
            locals,
        )?;
        let dst = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::GetSuperPropertyDynamic {
            dst,
            key,
            base: None,
        });
        return Some(dst);
    }
    let object =
        crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?;
    if !member.optional && !is_optional_chain_value(&member.object) {
        ops.push(Op::RequireObjectCoercible { src: object });
    }
    reduce_dynamic_get(
        &member.expression,
        object,
        ops,
        facts,
        next_register,
        locals,
    )
}
fn reduce_private_get(
    member: &oxc::ast::ast::PrivateFieldExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let name = facts.private_name(member.field.span)?;
    let object =
        crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetPrivate { dst, object, name });
    Some(dst)
}
fn emit_get(
    object_expression: &Expression<'_>,
    key: String,
    optional: bool,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(object_expression, Expression::Super(_)) {
        return Some(emit_super_get(ops, next_register, key));
    }
    let object =
        crate::reduce::reduce_expression(object_expression, ops, facts, next_register, locals)?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    let op = if optional || is_optional_chain_value(object_expression) {
        Op::OptionalGet {
            dst: register,
            object,
            key,
        }
    } else {
        Op::GetProperty {
            dst: register,
            object,
            key,
        }
    };
    ops.push(op);
    Some(register)
}

fn is_optional_chain_value(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ChainExpression(_) => true,
        Expression::ParenthesizedExpression(parenthesized) => {
            is_optional_chain_value(&parenthesized.expression)
        }
        Expression::StaticMemberExpression(member) => {
            member.optional || is_optional_chain_value(&member.object)
        }
        Expression::ComputedMemberExpression(member) => {
            member.optional || is_optional_chain_value(&member.object)
        }
        _ => false,
    }
}
fn emit_super_get(ops: &mut Vec<Op>, next_register: &mut u16, key: String) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetSuperProperty { dst, key });
    dst
}
fn reduce_dynamic_get(
    key_expression: &Expression<'_>,
    object: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let key = crate::reduce::reduce_expression(key_expression, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetPropertyDynamic { dst, object, key });
    Some(dst)
}
pub(crate) fn execute_get_dynamic(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::GetPropertyDynamic { dst, object, key } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let object_value = crate::execute::read_register(registers, *object)?;
    let key_value = crate::execute::read_register(registers, *key)?;
    let key = dynamic_property_key(&key_value)?;
    let value = crate::execute::get_property_result(&object_value, &key)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
pub(crate) fn execute_set_property(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    if try_plain_index_dynamic_write(registers, op)? {
        return Ok(());
    }
    let (object, key, src, strict) = set_property_parts(registers, op)?;
    let mut target = crate::execute::read_register(registers, object)?.clone();
    if matches!(
        target,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) && (crate::math::property(&key).is_some() || crate::math::constant(&key).is_some())
    {
        target = crate::vm::realm_intrinsic(crate::ops::Builtin::Math);
        crate::execute::write_value(registers, object, target.clone());
    }
    reject_nullish_property_write(&target)?;
    if crate::module_bindings::is_namespace(&target) {
        return write_failure(strict);
    }
    let value = unwrap_assignment_value(&crate::execute::read_register(registers, src)?);
    if crate::typed_array_ops::is_view(&target) {
        if crate::typed_array_ops::canonical_numeric_index(&key)
            && !crate::typed_array_ops::is_index_key(&key)
        {
            if !typed_array_buffer_detached(&target) {
                if matches!(
                    target,
                    crate::value::Value::BigInt64Array(_) | crate::value::Value::BigUint64Array(_)
                ) {
                    crate::construct::bigint_bits(&value)?;
                } else {
                    crate::conversion::to_number(&value)?;
                }
            }
            return Ok(());
        }
        if crate::typed_array_ops::is_index_key(&key) {
            if let Some(result) = crate::typed_array_ops::set_property(&target, &key, &value) {
                let updated = result?;
                crate::execute::write_value(registers, object, updated);
                return Ok(());
            }
        }
    }
    // Computed numeric writes are emitted as SetPropertyDynamic by the
    // general reducer. Keep the proven packed-array path ahead of extensible
    // and prototype checks, so each indexed append is constant work.
    if let (crate::value::Value::Array(array), Some(index), crate::value::Value::Number(number)) = (
        &target,
        crate::arrays::array_index(&key).map(|index| index as usize),
        &value,
    ) {
        if array.set_existing_f64(index, *number) || array.append_preallocated_f64(index, *number) {
            return Ok(());
        }
    }
    // Dynamic named writes on an ordinary object are common in builders. Once
    // the receiver proves an extensible/default-prototype shape and the key
    // is not already present, consult only that prototype's descriptor before
    // appending in place. Accessors, read-only inherited data, custom
    // prototypes, and existing properties retain the complete setter path.
    if let crate::value::Value::Object(object_data) = &target {
        if crate::builtins::object_alias::plain_named_write(object_data, &key)
            && object_data.hot_properties().position_rev(&key).is_none()
            && inherited_prototype_allows_plain_write(&target, &key)?
        {
            let updated =
                crate::builtins::object_alias::set(std::rc::Rc::clone(object_data), &key, value);
            crate::execute::write_value(registers, object, updated);
            return Ok(());
        }
    }
    if let crate::value::Value::Object(object_data) = &target {
        if crate::builtins::object_alias::plain_index_write(object_data, &key) {
            let updated =
                crate::builtins::object_alias::set(std::rc::Rc::clone(object_data), &key, value);
            crate::execute::write_value(registers, object, updated);
            return Ok(());
        }
    }
    let rejects_new = {
        let _scope = crate::execution_trace::attribution_scope("SetN:reject_new");
        rejects_new_property(&target, &key)
    };
    if rejects_new && key != "__proto__" {
        return write_failure(strict);
    }
    if matches!(target, crate::value::Value::Proxy(_)) {
        return assign_proxy_set(registers, object, &target, &key, value);
    }
    if key == "stack" && inherits_error_prototype(&target) {
        crate::vm::execute_builtin_with_receiver(
            crate::ops::Builtin::ErrorPrototypeStackSetter,
            &[value],
            Some(&target),
        )?;
        return Ok(());
    }
    let _scope = crate::execution_trace::attribution_scope("SetN:finish");
    finish_set_property(registers, object, &target, &key, value, strict)
}

fn inherited_prototype_allows_plain_write(
    target: &crate::value::Value,
    key: &str,
) -> Result<bool, crate::execute::VmError> {
    let prototype = crate::builtins::object::get_prototype_of(Some(target))?;
    let descriptor = crate::builtins::object::descriptor(
        Some(&prototype),
        Some(&crate::value::Value::String(key.to_owned())),
    )?;
    let crate::value::Value::Object(fields) = descriptor else {
        return Ok(true);
    };
    if fields
        .iter()
        .any(|(name, _)| name == "get" || name == "set")
    {
        return Ok(false);
    }
    let writable = fields.iter().any(|(name, value)| {
        name == "writable" && matches!(value, crate::value::Value::Boolean(true))
    });
    Ok(writable)
}

fn typed_array_buffer_detached(value: &crate::value::Value) -> bool {
    match value {
        crate::value::Value::Float64Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Float32Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Int8Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Int16Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Int32Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Uint8Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Uint8ClampedArray(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Uint16Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::Uint32Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::BigInt64Array(view) => *view.buffer.detached.borrow(),
        crate::value::Value::BigUint64Array(view) => *view.buffer.detached.borrow(),
        _ => false,
    }
}

fn try_plain_index_dynamic_write(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<bool, crate::execute::VmError> {
    let Op::SetPropertyDynamic {
        object, key, src, ..
    } = op
    else {
        return Ok(false);
    };
    let key_value = crate::execute::read_register(registers, *key)?;
    let crate::value::Value::Number(number) = key_value else {
        return Ok(false);
    };
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 {
        return Ok(false);
    }
    let index = number as u64;
    if index >= u64::from(u32::MAX) {
        return Ok(false);
    }
    let target = crate::execute::read_register(registers, *object)?;
    let crate::value::Value::Object(object_data) = &target else {
        return Ok(false);
    };
    let key = index.to_string();
    if !crate::builtins::object_alias::plain_index_write(object_data, &key) {
        return Ok(false);
    }
    let value = unwrap_assignment_value(&crate::execute::read_register(registers, *src)?);
    let updated = crate::builtins::object_alias::set(std::rc::Rc::clone(object_data), &key, value);
    crate::execute::write_value(registers, *object, updated);
    Ok(true)
}

/// Fast numeric indexed-write projection used by the compact ASetI handler.
/// The shape site is probed before slot derivation; every miss retains the
/// complete SetPropertyDynamic gateway as the semantic owner.  A successful
/// write installs the post-write layout, which matters when an indexed write
/// appends a new own property and changes the shape.
pub(crate) fn execute_set_index_number_cached(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    index: usize,
    number: f64,
    site: Option<&std::cell::RefCell<crate::quickening::QuickeningSite<4>>>,
) -> Result<bool, crate::execute::VmError> {
    let Some(crate::value::Value::Object(object_data)) = registers.read(usize::from(object)) else {
        return Ok(false);
    };
    if object_data.has_replacement()
        || object_data.is_realm_global()
        || object_data.is_script_global_view()
        || crate::regexp::has_regexp_internal_slot(&crate::value::Value::Object(
            std::rc::Rc::clone(&object_data),
        ))
    {
        return Ok(false);
    }
    let key = index.to_string();
    let target = crate::value::Value::Object(std::rc::Rc::clone(&object_data));
    let shape = crate::identity::ShapeId(object_data.semantic_layout_id());
    let property = crate::identity::property_key_id(&key);
    if let Some(site) = site {
        let cached_slot = site.borrow_mut().probe_shape(shape, property);
        if let Some(slot) = cached_slot {
            if let Some(word) = cached_plain_writable_slot(&object_data, &key, shape.0, slot) {
                word.store_number(number);
                return Ok(true);
            }
            site.borrow_mut().invalidate_shape(shape);
        }
    }
    if !crate::builtins::object_alias::plain_index_write(&object_data, &key)
        || rejects_new_property(&target, &key)
    {
        return Ok(false);
    }
    if !crate::builtins::object_alias::set_plain_index_number(&object_data, index, number) {
        return Ok(false);
    }
    if let Some(site) = site {
        if let Some(updated) = registers.read_object(usize::from(object)) {
            install_named_shape_site(site, updated, &key);
        }
    }
    Ok(true)
}

fn unwrap_assignment_value(value: &crate::value::Value) -> crate::value::Value {
    match value {
        crate::value::Value::BindingCell(cell) => unwrap_assignment_value(&cell.load()),
        crate::value::Value::WeakFunction(function) => function.value(),
        value => value.clone(),
    }
}

pub(crate) fn execute_set_named_cached(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    key: &str,
    src: u16,
    strict: bool,
    cache: &std::cell::Cell<u64>,
    site: Option<&std::cell::RefCell<crate::quickening::QuickeningSite<4>>>,
) -> Result<(), crate::execute::VmError> {
    // The shape site is the polymorphic key/state cache used by the baseline
    // path.  It is probed before any slot derivation; a failed proof falls
    // through to the complete setter and may install a new state afterward.
    if let Some(data) = registers.read_object(usize::from(object)) {
        if !data.has_replacement()
            && !data.has_regexp_internal_slot()
            && assignment_source_is_direct(registers, src)
        {
            let shape = crate::identity::ShapeId(data.semantic_layout_id());
            let property = crate::identity::property_key_id(key);
            let cached_slot = site.and_then(|site| site.borrow_mut().probe_shape(shape, property));
            if let Some(slot) = cached_slot {
                if let Some(word) =
                    cached_plain_writable_slot(data, key, data.semantic_layout_id(), slot)
                {
                    crate::execution_trace::event(
                        crate::execution_trace::Event::NamedPropertySetHit,
                    );
                    word.store_from_register(registers, usize::from(src))
                        .ok_or(crate::execute::VmError::MissingReturn)?;
                    return Ok(());
                }
                if let Some(site) = site {
                    site.borrow_mut().invalidate_shape(shape);
                }
            }
        }
    }
    let cached = cache.get();
    if cached & WRITE_TRANSITION_TAG == 0 {
        if let Some(data) = registers.read_object(usize::from(object)) {
            if !data.has_replacement()
                && !data.has_regexp_internal_slot()
                && assignment_source_is_direct(registers, src)
            {
                if let Some((layout, slot)) = crate::machine::unpack_named_cache(cached) {
                    if let Some(word) = cached_plain_writable_slot(&data, key, layout, slot) {
                        crate::execution_trace::event(
                            crate::execution_trace::Event::NamedPropertySetHit,
                        );
                        word.store_from_register(registers, usize::from(src))
                            .ok_or(crate::execute::VmError::MissingReturn)?;
                        return Ok(());
                    }
                }
            }
        }
    }
    let target = crate::execute::read_register(registers, object)?;
    let regexp_target = crate::regexp::has_regexp_internal_slot(&target);
    if !regexp_target
        && transition_index(cache.get()).is_some()
        && try_named_write_transition_attributed(registers, object, src, key, cache)?
    {
        if let Some(site) = site {
            if let Some(data) = registers.read_object(usize::from(object)) {
                install_named_shape_site(site, data, key);
            }
        }
        crate::execution_trace::event(crate::execution_trace::Event::NamedPropertySetHit);
        return Ok(());
    }
    if !regexp_target {
        if let crate::value::Value::Object(data) = &target {
            if data.has_replacement() {
                crate::execution_trace::event(crate::execution_trace::Event::NamedSetReplacement);
            } else if let Some((layout, slot)) = crate::machine::unpack_named_cache(cache.get()) {
                if let Some(word) = cached_plain_writable_slot(data, key, layout, slot)
                    .filter(|_| assignment_source_is_direct(registers, src))
                {
                    crate::execution_trace::event(
                        crate::execution_trace::Event::NamedPropertySetHit,
                    );
                    word.store_from_register(registers, usize::from(src))
                        .ok_or(crate::execute::VmError::MissingReturn)?;
                    return Ok(());
                } else if data.semantic_layout_id() != layout {
                    crate::execution_trace::event(
                        crate::execution_trace::Event::NamedSetLayoutMismatch,
                    );
                } else {
                    crate::execution_trace::event(
                        crate::execution_trace::Event::NamedSetSlotNotCell,
                    );
                }
            } else {
                crate::execution_trace::event(crate::execution_trace::Event::NamedSetCacheEmpty);
            }
        }
    }
    crate::execution_trace::event(crate::execution_trace::Event::NamedPropertySetMiss);
    {
        let _scope = crate::execution_trace::attribution_scope("SetN:slow");
        execute_set_property(
            registers,
            &Op::SetProperty {
                object,
                key: key.to_owned(),
                src,
                strict,
            },
        )?;
    }
    let transition = named_write_source(&target, key);
    let updated = crate::execute::read_register(registers, object)?;
    let transition_installed = install_named_write_transition(cache, transition, &updated, key);
    if !transition_installed {
        if let crate::value::Value::Object(data) = &updated {
            if let Some(slot) = cacheable_named_write_slot(data, key) {
                cache.set(crate::machine::pack_named_cache(
                    data.semantic_layout_id(),
                    slot,
                ));
            }
        }
    }
    if let crate::value::Value::Object(data) = &updated {
        if let Some(site) = site {
            install_named_shape_site(site, data, key);
        }
    }
    if transition_installed {
        return Ok(());
    }
    Ok(())
}

#[inline(always)]
fn try_named_write_transition_attributed(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    src: u16,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Result<bool, crate::execute::VmError> {
    let _scope = crate::execution_trace::attribution_scope("SetN:transition");
    try_named_write_transition(registers, object, src, key, cache)
}

fn cacheable_named_write_slot(data: &crate::value::ObjectData, key: &str) -> Option<u32> {
    if !plain_writable_own_data(data, key) {
        return None;
    }
    let slot = data.hot_properties().position_rev(key)?;
    // A binding cell is observable through closures and must be updated via
    // the complete setter, not replaced by the source register's wrapper.
    if matches!(
        data.hot_properties().slot_value(slot),
        Some(crate::value::Value::BindingCell(_))
    ) {
        return None;
    }
    u32::try_from(slot).ok()
}

#[inline(always)]
fn install_named_shape_site(
    site: &std::cell::RefCell<crate::quickening::QuickeningSite<4>>,
    data: &crate::value::ObjectData,
    key: &str,
) {
    let Some(slot) = cacheable_named_write_slot(data, key) else {
        return;
    };
    let shape = crate::identity::ShapeId(data.semantic_layout_id());
    let property = crate::identity::property_key_id(key);
    let _ = site.borrow_mut().observe(shape, property, slot);
}

#[inline(always)]
pub(crate) fn assignment_source_is_direct(
    registers: &crate::register_file::RegisterFile,
    source: u16,
) -> bool {
    !matches!(
        registers.read(usize::from(source)),
        Some(crate::value::Value::BindingCell(_)) | Some(crate::value::Value::WeakFunction(_))
    )
}

/// Validate every fact required by the direct named-write projection.  The
/// packed cache stores only layout and slot, so the canonical property vector
/// and descriptor metadata remain the authority for key identity and
/// writability.  Returning the physical word is safe only after those checks.
#[inline(always)]
fn cached_plain_writable_slot<'a>(
    data: &'a crate::value::ObjectData,
    key: &str,
    layout: u32,
    slot: u32,
) -> Option<&'a crate::register_file::SlotWord> {
    (data.semantic_layout_id() == layout).then_some(())?;
    let slot = usize::try_from(slot).ok()?;
    data.hot_properties()
        .name_at(slot)
        .is_some_and(|name| name == key)
        .then_some(())?;
    if !plain_writable_own_data(data, key) {
        return None;
    }
    if matches!(
        data.hot_properties().slot_value(slot),
        Some(crate::value::Value::BindingCell(_))
    ) {
        return None;
    }
    data.hot_properties().slot_word(slot)
}

/// Return a proven writable own-data slot for the typed property-store leaf.
/// Cache misses and transitions remain on the complete setter path.
pub(crate) fn proven_named_writable_slot(
    registers: &crate::register_file::RegisterFile,
    object: u16,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Option<*const crate::register_file::SlotWord> {
    if cache.get() & WRITE_TRANSITION_TAG != 0 {
        return None;
    }
    let data = registers.read_object(usize::from(object))?;
    if data.has_replacement()
        || data.has_regexp_internal_slot()
        || data.is_dictionary()
        || data.is_realm_global()
        || data.is_script_global_view()
    {
        return None;
    }
    let (layout, slot) = crate::machine::unpack_named_cache(cache.get())?;
    cached_plain_writable_slot(&data, key, layout, slot)
        .map(|word| word as *const crate::register_file::SlotWord)
}

fn finish_set_property(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    // A typed-array prototype consumes numeric keys before ordinary accessor
    // lookup.  Route this shape through the receiver-aware path so a setter
    // installed on `%TypedArray%.prototype` cannot observe the write.
    let parent = crate::builtins::object::get_prototype_of(Some(target))?;
    if parent.typed_array_meta().is_some()
        && (crate::typed_array_ops::is_index_key(key)
            || crate::typed_array_ops::canonical_numeric_index(key))
    {
        return ordinary_set(registers, object, target, key, value, strict);
    }
    let process_env = crate::execute::get_property(target, "\0quench:process_env")
        == crate::value::Value::Boolean(true);
    if process_env && key.is_empty() {
        return Ok(());
    }
    if process_env && crate::conversion::is_symbol_string(key) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert a Symbol value to a string",
        ));
    }
    if process_env && crate::conversion::is_symbol(&value) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert a Symbol value to a string",
        ));
    }
    if matches!(
        target,
        crate::value::Value::Builtin(crate::ops::Builtin::ObjectPrototype)
    ) && key == "__proto__"
    {
        crate::vm::execute_builtin_with_receiver(
            crate::ops::Builtin::ObjectPrototypeSetProto,
            std::slice::from_ref(&value),
            Some(target),
        )?;
        return Ok(());
    }
    let value = if process_env {
        crate::conversion::to_string(&value)
            .map(crate::value::Value::String)
            .unwrap_or(value)
    } else {
        value
    };
    let setter = {
        let _scope = crate::execution_trace::attribution_scope("SetN:accessor");
        if own_data_property(target, key) {
            None
        } else {
            crate::property_define::accessor(target, key, "set")
        }
    };
    if let Some(setter) = setter {
        if matches!(setter, crate::value::Value::Undefined) {
            return write_failure(strict);
        }
        crate::functions::execute_target(&setter, target, std::slice::from_ref(&value))?;
        if let Some(updated) = crate::locals::replacement(target) {
            crate::execute::write_value(registers, object, updated);
        }
        return Ok(());
    }
    let inherited_blocked = {
        let _scope = crate::execution_trace::attribution_scope("SetN:inherited");
        !own_data_property(target, key) && inherited_write_blocked(target, key)
    };
    if inherited_blocked {
        return write_failure(strict);
    }
    if crate::builtins::descriptor_flag(target, key, "writable") == Some(false) {
        return write_failure(strict);
    }
    if matches!(
        target,
        crate::value::Value::String(_)
            | crate::value::Value::StringUnits(_)
            | crate::value::Value::Number(_)
            | crate::value::Value::Boolean(_)
            | crate::value::Value::BigInt(_)
    ) {
        return finish_primitive_set(target, key, value, strict);
    }
    if let crate::value::Value::Builtin(builtin) = &target {
        if !crate::builtins::object::builtin_property_writable(*builtin, key) {
            return write_failure(strict);
        }
        return set_builtin_property(registers, object, target, key, value);
    }
    if let crate::value::Value::BoundFunction(bound) = &target {
        if crate::vm::is_intrinsic_bound(bound) {
            if let crate::value::Value::Builtin(builtin) = bound.target {
                if !crate::builtins::object::builtin_property_is_writable(builtin, key) {
                    return write_failure(strict);
                }
            }
        }
    }
    let _scope = crate::execution_trace::attribution_scope("SetN:ordinary");
    ordinary_set(registers, object, target, key, value, strict)
}

fn own_data_property(target: &crate::value::Value, key: &str) -> bool {
    if crate::builtins::object::has_own_property(
        Some(target),
        Some(&crate::value::Value::String(key.to_string())),
    ) != crate::value::Value::Boolean(true)
    {
        return false;
    }
    let Ok(descriptor) = crate::builtins::object::descriptor(
        Some(target),
        Some(&crate::value::Value::String(key.to_string())),
    ) else {
        return false;
    };
    matches!(descriptor, crate::value::Value::Object(fields) if fields.iter().any(|(name, _)| name == "value"))
}

fn inherits_error_prototype(target: &crate::value::Value) -> bool {
    if matches!(
        target,
        crate::value::Value::Builtin(
            crate::ops::Builtin::ErrorPrototype | crate::ops::Builtin::AggregateErrorPrototype,
        )
    ) {
        return true;
    }
    match target {
        crate::value::Value::Object(properties) => properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0prototype").then_some(value))
            .is_some_and(|target| inherits_error_prototype(&target)),
        crate::value::Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .and_then(|properties| {
                properties
                    .iter()
                    .rev()
                    .find_map(|(name, value)| (name == "\0prototype").then_some(value.clone()))
            })
            .is_some_and(|prototype| inherits_error_prototype(&prototype)),
        _ => false,
    }
}
fn set_property_parts(
    registers: &crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(u16, String, u16, bool), crate::execute::VmError> {
    match op {
        Op::SetProperty {
            object,
            key,
            src,
            strict,
        } => Ok((*object, key.clone(), *src, *strict)),
        Op::SetPropertyDynamic {
            object,
            key,
            src,
            strict,
        } => Ok((
            *object,
            dynamic_property_key(&crate::execute::read_register(registers, *key)?)?,
            *src,
            *strict,
        )),
        _ => Err(crate::execute::VmError::MissingReturn),
    }
}
fn finish_primitive_set(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    // Per V8 / ES engine behaviour, assignment to a Symbol primitive
    // value is rejected outright: in strict mode it throws a TypeError;
    // in non-strict mode the write is silently dropped (the auto-boxed
    // wrapper is discarded before any subsequent read).
    if strict && crate::conversion::is_symbol(target) {
        return Err(crate::value::error::throw_type_error(
            "Cannot create property on a symbol value",
        ));
    }
    let receiver = crate::construct::to_object(target)?;
    let mut home_proto = primitive_prototype_for(target);
    loop {
        match &home_proto {
            crate::value::Value::Null => break,
            crate::value::Value::Proxy(_) => {
                let trap_result =
                    crate::proxy::proxy_set(&home_proto, key, &value, Some(&receiver))?;
                let succeeded = matches!(trap_result, crate::value::Value::Boolean(true));
                if !succeeded && strict {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot assign to read-only property",
                    ));
                }
                return Ok(());
            }
            _ => {}
        }
        let descriptor = crate::builtins::object::descriptor(
            Some(&home_proto),
            Some(&crate::value::Value::String(key.to_string())),
        )?;
        if !matches!(descriptor, crate::value::Value::Undefined) {
            let has_setter = !matches!(
                crate::builtins::object::descriptor(
                    Some(&descriptor),
                    Some(&crate::value::Value::String("set".to_string())),
                ),
                Ok(crate::value::Value::Undefined)
            );
            if has_setter {
                let succeeded = crate::proxy::proxy_set(&home_proto, key, &value, Some(&receiver))?;
                let ok = matches!(succeeded, crate::value::Value::Boolean(true));
                if !ok && strict {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot assign to read-only property",
                    ));
                }
                return Ok(());
            }
            let writable = matches!(
                crate::execute::get_property_result(&descriptor, "writable")?,
                crate::value::Value::Boolean(true)
            );
            if !writable {
                if strict {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot assign to read-only property",
                    ));
                }
                return Ok(());
            }
            let own = vec![
                ("value".to_string(), value),
                ("writable".to_string(), crate::value::Value::Boolean(true)),
                ("enumerable".to_string(), crate::value::Value::Boolean(true)),
                (
                    "configurable".to_string(),
                    crate::value::Value::Boolean(true),
                ),
            ];
            let _ = crate::builtins::define_own_property(&receiver, key, &own)?;
            return Ok(());
        }
        home_proto = crate::builtins::object::get_prototype_of(Some(&home_proto))?;
    }
    let own = vec![
        ("value".to_string(), value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        ("enumerable".to_string(), crate::value::Value::Boolean(true)),
        (
            "configurable".to_string(),
            crate::value::Value::Boolean(true),
        ),
    ];
    let _ = crate::builtins::define_own_property(&receiver, key, &own)?;
    Ok(())
}

fn primitive_prototype_for(value: &crate::value::Value) -> crate::value::Value {
    use crate::ops::Builtin;
    use crate::value::Value;
    match value {
        Value::Number(_) => crate::vm::realm_intrinsic(Builtin::NumberPrototype),
        Value::Boolean(_) => crate::vm::realm_intrinsic(Builtin::BooleanPrototype),
        Value::StringUnits(_) => crate::vm::realm_intrinsic(Builtin::StringPrototype),
        Value::BigInt(_) => crate::vm::realm_intrinsic(Builtin::BigIntPrototype),
        Value::String(v) if crate::conversion::is_symbol_string(v) => {
            crate::vm::realm_intrinsic(Builtin::SymbolPrototype)
        }
        Value::String(_) => crate::vm::realm_intrinsic(Builtin::StringPrototype),
        _ => Value::Null,
    }
}
fn reject_nullish_property_write(
    target: &crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    if matches!(
        target,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign a property of null or undefined",
        ));
    }
    Ok(())
}
include!("properties_function_name.rs");

fn set_builtin_property(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    // Assignment to an existing property updates only its value; attributes
    // are preserved by complete_descriptor. New properties get the
    // assignment defaults (writable, enumerable, configurable).
    let key_value = crate::value::Value::String(key.to_string());
    let existing = crate::builtins::object::descriptor(Some(target), Some(&key_value))?;
    let exists = !matches!(existing, crate::value::Value::Undefined);
    let mut fields = vec![("value".to_string(), value)];
    if !exists {
        for name in ["writable", "enumerable", "configurable"] {
            fields.push((name.to_string(), crate::value::Value::Boolean(true)));
        }
    }
    let updated = crate::builtins::define_own_property(target, key, &fields)?;
    crate::execute::write_value(registers, object, updated);
    Ok(())
}

pub(crate) fn rejects_new_property(target: &crate::value::Value, key: &str) -> bool {
    match target {
        crate::value::Value::Object(properties) => marked_without_key(properties.as_ref(), key),
        crate::value::Value::Function(function) => {
            let properties = function.properties.borrow();
            marked_without_key(&properties[..], key)
        }
        crate::value::Value::BoundFunction(bound) => {
            let properties = bound.properties.borrow();
            marked_without_key(&properties[..], key)
        }
        crate::value::Value::Array(values) => {
            let own = key == "length"
                || crate::arrays::array_index(key)
                    .is_some_and(|index| values.has_index(index as usize))
                || values.property(key).is_some();
            values.property(NON_EXTENSIBLE).is_some() && !own
        }
        _ => false,
    }
}

fn marked_without_key<P: crate::value::PropertyEntries + ?Sized>(
    properties: &P,
    key: &str,
) -> bool {
    properties.entries().any(|(name, _)| name == NON_EXTENSIBLE)
        && !properties.entries().any(|(name, _)| name == key)
}

pub(crate) fn object_is_extensible(target: &crate::value::Value) -> bool {
    let target = crate::locals::resolved_replacement(target.clone());
    if let crate::value::Value::BindingCell(cell) = &target {
        return object_is_extensible(&cell.borrow());
    }
    if let Some(meta) = target.typed_array_meta() {
        return meta.is_extensible();
    }
    match &target {
        crate::value::Value::Builtin(crate::ops::Builtin::ThrowTypeError) => false,
        crate::value::Value::Builtin(builtin) => {
            !crate::builtins::builtin_is_non_extensible(*builtin)
        }
        crate::value::Value::Object(properties) => {
            !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE)
        }
        crate::value::Value::Array(values) => values.property(NON_EXTENSIBLE).is_none(),
        crate::value::Value::Function(function) => !function
            .properties
            .borrow()
            .iter()
            .any(|(name, _)| name == NON_EXTENSIBLE),
        crate::value::Value::BoundFunction(bound) => !bound
            .properties
            .borrow()
            .iter()
            .any(|(name, _)| name == NON_EXTENSIBLE),
        crate::value::Value::Set(data) => data.is_extensible(),
        value => crate::value::is_object(value),
    }
}

pub(crate) fn is_extensible_value(
    target: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let target = target.ok_or(crate::execute::VmError::NotCallable)?;
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_is_extensible(target);
    }
    Ok(crate::value::Value::Boolean(object_is_extensible(target)))
}

include!("properties_integrity.rs");

pub(crate) fn prevent_extensions(
    target: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let Some(target) = target else {
        return Err(crate::value::error::throw_type_error("Object expected"));
    };
    if let crate::value::Value::BindingCell(cell) = target {
        let current = cell.load();
        let updated = prevent_extensions(Some(&current))?;
        cell.store(updated);
        return Ok(target.clone());
    }
    if matches!(target, crate::value::Value::Proxy(_)) {
        let result = crate::proxy::proxy_prevent_extensions(target)?;
        if !crate::execute::is_truthy(&result) {
            return Err(crate::value::error::throw_type_error(
                "Proxy preventExtensions returned false",
            ));
        }
        return Ok(target.clone());
    }
    let result = mark_non_extensible(target);
    crate::locals::replace_value(target, &result);
    if crate::vm::is_global_object(target) {
        let mut registers = crate::register_file::RegisterFile::new();
        crate::vm::synchronize_global_object(&mut registers, target, &result);
    }
    Ok(result)
}

fn mark_non_extensible(target: &crate::value::Value) -> crate::value::Value {
    match target {
        crate::value::Value::Builtin(builtin) => {
            crate::builtins::mark_builtin_non_extensible(*builtin);
            target.clone()
        }
        crate::value::Value::Object(properties) => {
            let mut sealed = properties.as_ref().clone();
            push_non_extensible(&mut sealed);
            let next = crate::value::Value::Object(std::rc::Rc::new(sealed));
            crate::module_bindings::rehome_evaluator(target, &next);
            next
        }
        crate::value::Value::Array(values) => {
            let mut values = std::rc::Rc::clone(values);
            std::rc::Rc::make_mut(&mut values)
                .set_property(NON_EXTENSIBLE, crate::value::Value::Boolean(true));
            crate::value::Value::Array(values)
        }
        crate::value::Value::Function(function) => {
            mark_properties(&mut function.properties.borrow_mut());
            target.clone()
        }
        crate::value::Value::BoundFunction(bound) => {
            mark_properties(&mut bound.properties.borrow_mut());
            target.clone()
        }
        target if target.typed_array_meta().is_some() => {
            if let Some(meta) = target.typed_array_meta() {
                meta.set_extensible(false);
            }
            target.clone()
        }
        _ => target.clone(),
    }
}

fn mark_properties(properties: &mut Vec<(String, crate::value::Value)>) {
    if !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE) {
        properties.push((
            NON_EXTENSIBLE.to_string(),
            crate::value::Value::Boolean(true),
        ));
    }
}

fn reject_restricted_property_write(
    target: &crate::value::Value,
    key: &str,
) -> Result<(), crate::execute::VmError> {
    if matches!(&target, crate::value::Value::Array(values) if values.is_strict_arguments() && key == "callee")
    {
        return Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        ));
    }
    if crate::vm::has_restricted_function_property(target, key) {
        return Err(crate::value::error::throw_type_error(
            "'caller' and 'arguments' are unavailable on this function",
        ));
    }
    Ok(())
}

pub(crate) fn inherited_write_blocked(target: &crate::value::Value, key: &str) -> bool {
    // Prototype objects do not truly own `length`/`name`; assigning them
    // creates an own property that shadows the callable metadata.
    let prototype_meta_key = matches!(key, "length" | "name")
        && matches!(target, crate::value::Value::Builtin(builtin) if crate::builtin_meta::is_prototype(*builtin));
    if !prototype_meta_key
        && crate::builtins::descriptor_flag(target, key, "writable") == Some(false)
    {
        return true;
    }
    if matches!(
        crate::property_define::accessor(target, key, "writable"),
        Some(crate::value::Value::Boolean(false))
    ) {
        return true;
    }
    let mut prototype = crate::builtins::object::get_prototype_of(Some(target)).ok();
    while let Some(value) = prototype {
        if matches!(
            value,
            crate::value::Value::Null | crate::value::Value::Undefined
        ) {
            break;
        }
        let descriptor = crate::builtins::object::descriptor(
            Some(&value),
            Some(&crate::value::Value::String(key.to_string())),
        )
        .ok();
        if let Some(crate::value::Value::Object(fields)) = descriptor {
            let field = |name: &str| {
                fields
                    .iter()
                    .rev()
                    .find_map(|(field, value)| (field == name).then_some(value.clone()))
            };
            if matches!(field("writable"), Some(crate::value::Value::Boolean(false))) {
                return true;
            }
            if fields.iter().any(|(name, _)| name == "set")
                && matches!(field("set"), Some(crate::value::Value::Undefined) | None)
            {
                return true;
            }
            return false;
        }
        prototype = crate::builtins::object::get_prototype_of(Some(&value)).ok();
    }
    false
}
fn write_failure(strict: bool) -> Result<(), crate::execute::VmError> {
    if strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    Ok(())
}

include!("properties_assign.rs");
include!("properties_copy_data.rs");
include!("properties_reflect_set.rs");

#[cfg(test)]
mod named_write_cache_tests {
    use super::execute_set_named_cached;
    use crate::{
        machine,
        register_file::RegisterFile,
        value::{BindingCell, ObjectData, Value},
    };
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn cached_write_does_not_replace_a_binding_cell() {
        let cell = BindingCell::new(Value::Number(1.0));
        let object = Rc::new(ObjectData::new(vec![(
            "field".to_owned(),
            Value::BindingCell(Rc::clone(&cell)),
        )]));
        let layout = object.semantic_layout_id();
        let mut registers =
            RegisterFile::from_values(vec![Value::Object(Rc::clone(&object)), Value::Number(9.0)]);
        let cache = Cell::new(machine::pack_named_cache(layout, 0));

        execute_set_named_cached(&mut registers, 0, "field", 1, false, &cache, None)
            .expect("binding-cell write");

        assert_eq!(cell.load(), Value::Number(9.0));
        assert!(matches!(
            object.hot_properties().slot_value(0),
            Some(Value::BindingCell(_))
        ));
    }

    #[test]
    fn cached_write_unwraps_a_binding_cell_source() {
        let object = Rc::new(ObjectData::new(vec![(
            "field".to_owned(),
            Value::Number(1.0),
        )]));
        let layout = object.semantic_layout_id();
        let source = BindingCell::new(Value::Number(9.0));
        let mut registers = RegisterFile::from_values(vec![
            Value::Object(Rc::clone(&object)),
            Value::BindingCell(Rc::clone(&source)),
        ]);
        let cache = Cell::new(machine::pack_named_cache(layout, 0));

        execute_set_named_cached(&mut registers, 0, "field", 1, false, &cache, None)
            .expect("unwrapped source write");

        assert_eq!(
            object.hot_properties().slot_value(0),
            Some(Value::Number(9.0))
        );
    }

    #[test]
    fn named_write_site_retains_bounded_polymorphic_shapes() {
        let first = Rc::new(ObjectData::new(vec![(
            "field".to_owned(),
            Value::Number(1.0),
        )]));
        let second = Rc::new(ObjectData::new(vec![
            ("other".to_owned(), Value::Number(2.0)),
            ("field".to_owned(), Value::Number(3.0)),
        ]));
        let site = std::cell::RefCell::new(crate::quickening::QuickeningSite::<4>::new(
            crate::ir::Opcode::SetN,
        ));
        let cache = Cell::new(0);
        let mut registers =
            RegisterFile::from_values(vec![Value::Object(Rc::clone(&first)), Value::Number(10.0)]);

        execute_set_named_cached(&mut registers, 0, "field", 1, false, &cache, Some(&site))
            .expect("first shape write");
        registers.write(0, Value::Object(Rc::clone(&second)));
        registers.write(1, Value::Number(20.0));
        execute_set_named_cached(&mut registers, 0, "field", 1, false, &cache, Some(&site))
            .expect("second shape write");

        assert_eq!(site.borrow().cache_len(), 2);
        registers.write(0, Value::Object(Rc::clone(&first)));
        registers.write(1, Value::Number(30.0));
        execute_set_named_cached(&mut registers, 0, "field", 1, false, &cache, Some(&site))
            .expect("polymorphic hit");
        assert_eq!(
            first.hot_properties().slot_value(0),
            Some(Value::Number(30.0))
        );
        assert_eq!(
            second.hot_properties().slot_value(1),
            Some(Value::Number(20.0))
        );
    }
}

include!("properties_delete.rs");
include!("properties_methods.rs");
include!("properties_prototype.rs");
