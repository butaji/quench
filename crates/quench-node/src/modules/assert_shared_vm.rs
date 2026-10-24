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
const ASYNC_REJECTS_STACK: &str = "\0builtin:assert/async-rejects-stack";

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

/// The successful strict scalar path has no message coercion or option reads.
/// SameValue belongs to Core; all failed and missing-argument cases retain
/// the configured library's error, custom-message and stack policy.
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
    for (name, equal) in [("strictEqual", true), ("notStrictEqual", false)] {
        let original = get(context, module, name)?;
        if !context.is_callable_rooted(original)? {
            continue;
        }
        let operation = if equal {
            "assertStrictEqual"
        } else {
            "assertNotStrictEqual"
        };
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
    install_async_rejects_stack(context, module)?;
    let retained = context.retain(module)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(SCALAR_GUARDS.to_owned(), retained);
    Ok(())
}

fn install_async_rejects_stack(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    if context
        .host_mut()
        .shared_state()
        .borrow()
        .builtin_cache
        .contains_key(ASYNC_REJECTS_STACK)
    {
        return Ok(());
    }
    let source = r#"((assertModule) => {
      function patchRejects(target) {
        if (!target) return;
        const descriptor = Object.getOwnPropertyDescriptor(target, 'rejects');
        if (!descriptor || typeof descriptor.value !== 'function') return;
        const original = descriptor.value;
        function rejects(...args) {
          return Reflect.apply(original, this, args).catch((error) => {
            // The engine cannot reconstruct V8's async parent frames after an
            // await. Keep Node's visible async `rejects` frame when the
            // assertion stack was otherwise trimmed to only its message.
            if (error?.code === 'ERR_ASSERTION' &&
                error.operator === 'rejects' &&
                typeof error.stack === 'string' &&
                !error.stack.includes('rejects')) {
              error.stack += '\n    at async rejects';
            }
            throw error;
          });
        }
        Object.defineProperty(rejects, 'name', { value: original.name, configurable: true });
        Object.defineProperty(rejects, 'length', { value: original.length });
        Object.defineProperty(target, 'rejects', { ...descriptor, value: rejects });
      }
      patchRejects(assertModule);
      patchRejects(assertModule.strict);
    })"#;
    let installer = context.evaluate_script_rooted(source, "assert/async-rejects-stack.js")?;
    let undefined = context.undefined();
    context.call_rooted(installer, undefined, &[module])?;
    let retained = context.retain(module)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(ASYNC_REJECTS_STACK.to_owned(), retained);
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
