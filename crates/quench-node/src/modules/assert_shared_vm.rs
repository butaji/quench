//! Canonical configured Node assertion and comparison policies.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
) -> Result<RootId, RootedError> {
    let module =
        crate::host::shared_vm::commonjs::node_library_module(context, "assert.js", "assert.js")?;
    install_scalar_guards(context, module)?;
    Ok(module)
}

const SCALAR_GUARDS: &str = "\0builtin:assert/scalar-guards";

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}

/// The guarded assertions and the host operation each one's wrapper runs.
const GUARDS: [(&str, &str); 4] = [
    ("strictEqual", "assertStrictEqual"),
    ("notStrictEqual", "assertNotStrictEqual"),
    ("deepStrictEqual", "assertDeepEqual"),
    ("deepEqual", "assertDeepEqual"),
];

/// The successful scalar and plain-data paths have no message coercion or option reads.
/// SameValue and plain-data equality belong to Core; all failed and missing-argument cases
/// retain the configured library's error, custom-message and stack policy.
fn install_scalar_guards(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    if context
        .host_mut()
        .shared_state()
        .borrow()
        .builtin_cache
        .contains_key(SCALAR_GUARDS)
    {
        return Ok(());
    }
    let strict = get(context, module, "strict")?;
    if !context.is_callable_rooted(strict)? {
        return Ok(());
    }
    let constructor = get(context, module, "Assert")?;
    let mut holders = vec![module, strict];
    if context.is_callable_rooted(constructor)? {
        holders.push(get(context, constructor, "prototype")?);
    }
    for (name, operation) in GUARDS {
        let original = get(context, module, name)?;
        if !context.is_callable_rooted(original)? {
            continue;
        }
        let wrapper = context
            .host_constructor_with_data(crate::host::shared_vm::operation(operation), original)?;
        for property in ["name", "length"] {
            let value = get(context, original, property)?;
            let key = context.string_rooted(property);
            context.define_data_property_rooted(wrapper, key, value, false, false, true)?;
        }
        for &holder in &holders {
            for key in context.own_keys_rooted(holder)? {
                let descriptor = context.own_property_descriptor_rooted(holder, key)?;
                let value = get(context, descriptor, "value")?;
                if context.same_value_rooted(value, original)? {
                    let value_key = context.string_rooted("value");
                    context.set_property_rooted(descriptor, value_key, wrapper, descriptor)?;
                    context.define_property_descriptor_rooted(holder, key, descriptor)?;
                }
            }
        }
    }
    let retained = context.retain(module)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(SCALAR_GUARDS.to_owned(), retained);
    Ok(())
}

pub(crate) fn strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    scalar_guard::<true>(context, receiver, args)
}

pub(crate) fn not_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    scalar_guard::<false>(context, receiver, args)
}

fn scalar_guard<const EQUAL: bool>(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() >= 2 && context.same_value_rooted(args[0], args[1])? == EQUAL {
        return Ok(context.undefined());
    }
    let original = context.host_function_data()?;
    context.call_rooted(original, receiver, args)
}

/// `deepStrictEqual` and the legacy `deepEqual`: plain data that Core proves the same
/// (`plain_data_equal_rooted`) passes both, since Node's comparison of such data branches only
/// on what that proof fixed (`lib/internal/util/comparisons.js`: the intrinsic constructor and
/// tag, the enumerable string keys and, per member, SameValue or the same comparison), and
/// the legacy one relaxes it further. Everything else runs the library's assertion.
pub(crate) fn deep_equal(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() >= 2 && context.plain_data_equal_rooted(args[0], args[1])? {
        return Ok(context.undefined());
    }
    let original = context.host_function_data()?;
    context.call_rooted(original, receiver, args)
}
