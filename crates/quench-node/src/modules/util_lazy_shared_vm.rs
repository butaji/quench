//! Node's lazy data properties: a property that reads a library module's export on first
//! access (SetLazyDataProperty), and the globals the bootstrap defines that way.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use quench_runtime::{NativeContext, RootId, RootedError};

/// Node's native SetLazyDataProperty protocol, using Core's genuine lazy data slots.
pub(crate) fn define_lazy_properties(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let [target, id, keys, options @ ..] = args else {
        return Err(RootedError::host(
            "defineLazyProperties requires target, id and keys",
        ));
    };
    let length = get(context, *keys, "length")?;
    let length = context.to_number(length)? as usize;
    let enumerable = match options.first() {
        Some(value) => context.truthy_rooted(*value)?,
        None => true,
    };
    // Each key reads the same-named export.
    for index in 0..length {
        let key = get(context, *keys, &index.to_string())?;
        define_lazy_export(context, *target, *id, key, key, enumerable)?;
    }
    Ok(context.undefined())
}

/// `property` on `target` reads export `export` of library module `id` on first access.
fn define_lazy_export(
    context: &mut NativeContext<'_, NodeHost>, target: RootId, id: RootId, property: RootId, export: RootId, enumerable: bool,
) -> Result<(), RootedError> {
    let name = context.to_string(property)?;
    let flag = context.boolean(enumerable);
    let data = context.array_rooted(&[target, id, property, flag, export])?;
    let initializer = context.host_function_with_data(
        crate::host::shared_vm::operation!("utilLazyPropertyInitialize"), data,
    )?;
    context.define_lazy_data_properties_rooted(target, &[&name], enumerable, initializer)
}

/// Globals Node's bootstrap defines lazily from its library, each `(property, module,
/// export, enumerable)`: `defineLazyProperties(globalThis, 'buffer', ['atob', 'btoa'])`
/// and `exposeLazyInterfaces` for the URL interfaces. The `__node*` names are
/// the host's own aliases of the original interfaces.
const LAZY_GLOBALS: &[(&str, &str, &str, bool)] = &[
    ("atob", "buffer", "atob", true),
    ("btoa", "buffer", "btoa", true),
    ("URL", "internal/url", "URL", false),
    ("URLSearchParams", "internal/url", "URLSearchParams", false),
    ("__nodeURL", "internal/url", "URL", false),
    ("__nodeURLSearchParams", "internal/url", "URLSearchParams", false),
];

#[cfg(feature = "aot")]
pub(crate) fn lazy_global_names() -> impl Iterator<Item = &'static str> {
    LAZY_GLOBALS.iter().map(|&(property, _, _, _)| property)
}

pub(crate) fn install_lazy_globals(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    for &(property, id, export, enumerable) in LAZY_GLOBALS {
        let property = context.string_rooted(property);
        let id = context.string_rooted(id);
        let export = context.string_rooted(export);
        define_lazy_export(context, global, id, property, export, enumerable)?;
    }
    Ok(())
}

pub(crate) fn initialize_lazy_property(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let target = get(context, data, "0")?;
    let id = get(context, data, "1")?;
    let key = get(context, data, "2")?;
    let flag = get(context, data, "3")?;
    let export = get(context, data, "4")?;
    let export = if context.rooted_value(export).is_some_and(|value| value.is_undefined()) { key } else { export };
    let enumerable = context.truthy_rooted(flag)?;
    let id = context.to_string(id)?;
    let module = crate::host::shared_vm::commonjs::builtin_require(context, &id)?;
    let value = context.get_property_rooted(module, export)?;
    context.define_data_property_rooted(target, key, value, true, enumerable, true)?;
    Ok(context.undefined())
}

