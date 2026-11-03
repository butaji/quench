//! `internal/bootstrap/realm` under `--expose-internals`: Node's `BuiltinModule` queries
//! over the public builtin list (`module_shared_vm::BUILTIN_MODULES`), and `internalBinding`.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

const SCHEME: &str = "node:";

/// `BuiltinModule`'s static queries with their operations.
const QUERIES: [(&str, &str); 7] = [
    ("exists", "realmBuiltinByUsers"),
    ("canBeRequiredByUsers", "realmBuiltinByUsers"),
    ("canBeRequiredWithoutScheme", "realmBuiltinWithoutScheme"),
    ("normalizeRequirableId", "realmBuiltinNormalize"),
    ("isBuiltin", "realmBuiltinIsBuiltin"),
    ("getSchemeOnlyModuleNames", "realmBuiltinSchemeOnly"),
    ("getAllBuiltinModuleIds", "realmBuiltinAll"),
];

/// Every public builtin id, and whether it is only requirable with the `node:` scheme.
fn ids() -> impl Iterator<Item = (&'static str, bool)> {
    super::module_shared_vm::BUILTIN_MODULES
        .iter()
        .map(|name| name.strip_prefix(SCHEME).map_or((*name, false), |name| (name, true)))
}

fn by_users(id: &str) -> bool {
    ids().any(|(name, _)| name == id)
}

fn without_scheme(id: &str) -> bool {
    ids().any(|(name, scheme_only)| name == id && !scheme_only)
}

/// `normalizeRequirableId(id)`: the id a `node:` specifier or bare builtin names.
fn normalize(id: &str) -> Option<&str> {
    match id.strip_prefix(SCHEME) {
        Some(name) if by_users(name) => Some(name),
        Some(_) => None,
        None => without_scheme(id).then_some(id),
    }
}

pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let class = c.host_constructor_named(crate::host::shared_vm::operation!("processConstructor"), "BuiltinModule")?;
    for (name, operation_name) in QUERIES {
        let function = c.host_function_named(operation(operation_name), name)?;
        c.assign_named_property_rooted(class, name, function)?;
    }
    let module = c.object_rooted()?;
    c.assign_named_property_rooted(module, "BuiltinModule", class)?;
    let binding = c.host_function_named(crate::host::shared_vm::operation!("traceInternal"), "internalBinding")?;
    c.assign_named_property_rooted(module, "internalBinding", binding)?;
    Ok(module)
}

fn id(c: &mut Context<'_>, args: &[RootId]) -> Result<Option<String>, RootedError> {
    match args.first() {
        Some(id) => c.string_text(*id),
        None => Ok(None),
    }
}

fn answer(c: &mut Context<'_>, args: &[RootId], query: fn(&str) -> bool) -> Result<RootId, RootedError> {
    let answer = id(c, args)?.is_some_and(|id| query(&id));
    Ok(c.boolean(answer))
}

pub(crate) fn can_be_required_by_users(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    answer(c, args, by_users)
}

pub(crate) fn can_be_required_without_scheme(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    answer(c, args, without_scheme)
}

pub(crate) fn is_builtin(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    answer(c, args, |id| normalize(id).is_some())
}

pub(crate) fn normalize_requirable_id(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    match id(c, args)?.as_deref().and_then(normalize) {
        Some(id) => Ok(c.string_rooted(id)),
        None => Ok(c.undefined()),
    }
}

fn names(c: &mut Context<'_>, keep: fn(bool) -> bool) -> Result<RootId, RootedError> {
    let names = ids().filter(|(_, scheme_only)| keep(*scheme_only)).map(|(name, _)| c.string_rooted(name)).collect::<Vec<_>>();
    c.array_rooted(&names)
}

pub(crate) fn scheme_only_names(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    names(c, |scheme_only| scheme_only)
}

pub(crate) fn all_ids(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    names(c, |_| true)
}
