//! Original SDK test scheduling, contexts, hooks and mock policy.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let harness = crate::host::shared_vm::commonjs::node_library_module(
        context, "test.js", "internal/test_runner/harness.js",
    )?;
    let test = get(context, harness, "test")?;
    // The original harness owns every function and its lifecycle. File-runner
    // exports still need a genuine native V8 serializer and are not fabricated.
    for (name, original) in [
        ("after", "after"), ("afterEach", "afterEach"), ("before", "before"),
        ("beforeEach", "beforeEach"), ("describe", "suite"),
        ("getTestContext", "getTestContext"), ("it", "test"),
        ("suite", "suite"), ("test", "test"),
    ] {
        let value = get(context, harness, original)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(test, key, value, true, true, true)?;
    }
    accessor(context, test, "mock", "nodeTestMock", None)?;
    accessor(context, test, "snapshot", "nodeTestNamespace", Some((
        "internal/test_runner/snapshot.js", &["setDefaultSnapshotSerializers", "setResolveSnapshotPath"],
    )))?;
    accessor(context, test, "assert", "nodeTestNamespace", Some((
        "internal/test_runner/assert.js", &["register"],
    )))?;
    Ok(test)
}

fn accessor(
    context: &mut Context<'_>, target: RootId, name: &str, operation: &str,
    namespace: Option<(&str, &[&str])>,
) -> Result<(), RootedError> {
    let cache = context.object_rooted()?;
    if let Some((path, names)) = namespace {
        let path = context.string_rooted(path);
        let key = context.string_rooted("path");
        context.define_data_property_rooted(cache, key, path, true, true, true)?;
        let names = names.iter().map(|name| context.string_rooted(name)).collect::<Vec<_>>();
        let names = context.array_rooted(&names)?;
        let key = context.string_rooted("names");
        context.define_data_property_rooted(cache, key, names, true, true, true)?;
    }
    let getter = context.host_function_named_with_data(
        crate::host::shared_vm::operation(operation), "get", cache,
    )?;
    let descriptor = context.null_object_rooted()?;
    for (name, value) in [
        ("get", getter), ("enumerable", context.boolean(true)), ("configurable", context.boolean(true)),
    ] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(descriptor, key, value, true, true, true)?;
    }
    let key = context.string_rooted(name);
    context.define_property_descriptor_rooted(target, key, descriptor)?;
    Ok(())
}

pub(crate) fn mock(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let cache = context.host_function_data()?;
    let current = get(context, cache, "value")?;
    if context.is_object_rooted(current)? { return Ok(current); }
    let constructor = crate::host::shared_vm::commonjs::node_library_export(
        context, "test.js", "internal/test_runner/mock/mock.js", "MockTracker",
    )?;
    let value = context.construct_rooted(constructor, constructor, &[])?;
    let key = context.string_rooted("value");
    context.define_data_property_rooted(cache, key, value, true, true, true)?;
    Ok(value)
}

pub(crate) fn namespace(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let cache = context.host_function_data()?;
    let current = get(context, cache, "value")?;
    if context.is_object_rooted(current)? { return Ok(current); }
    let path = get(context, cache, "path")?;
    let path = context.to_string(path)?;
    let library = crate::host::shared_vm::commonjs::node_library_module(context, "test.js", &path)?;
    let names = get(context, cache, "names")?;
    let length = get(context, names, "length")?;
    let length = context.to_number(length)? as usize;
    let value = context.null_object_rooted()?;
    for index in 0..length {
        let key = get(context, names, &index.to_string())?;
        let method = context.get_property_rooted(library, key)?;
        context.define_data_property_rooted(value, key, method, true, true, true)?;
    }
    let key = context.string_rooted("value");
    context.define_data_property_rooted(cache, key, value, true, true, true)?;
    Ok(value)
}
