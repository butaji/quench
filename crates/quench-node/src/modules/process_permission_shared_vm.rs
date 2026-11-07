//! `process.permission`, Node's Permission Model API: present only under `--permission`
//! or `--permission-audit`, answering from the grants of Node's normalized startup options.
use super::event_constructor_shared_vm::get;
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::cell::RefCell;

type Context<'a> = NativeContext<'a, NodeHost>;

const PERMISSION: &str = "permission";
const ENABLE: &str = "--permission";
const AUDIT: &str = "--permission-audit";
/// A path grant that grants every path, and a path grant's wildcard.
const ANY_PATH: &str = "*";
const WILDCARD: char = '*';
const FILE_SCHEME: &str = "file:";

/// The model's scopes: name, Node's permission name, its diagnostics channel suffix and
/// the boolean flag that grants it (path scopes take paths instead).
struct Scope {
    name: &'static str,
    permission: &'static str,
    channel: &'static str,
    flag: Option<&'static str>,
}
const SCOPES: [Scope; 10] = [
    Scope { name: "fs.read", permission: "FileSystemRead", channel: "fs", flag: None },
    Scope { name: "fs.write", permission: "FileSystemWrite", channel: "fs", flag: None },
    Scope { name: "child", permission: "ChildProcess", channel: "child", flag: Some("--allow-child-process") },
    Scope { name: "worker", permission: "WorkerThreads", channel: "worker", flag: Some("--allow-worker") },
    Scope { name: "net", permission: "Net", channel: "net", flag: Some("--allow-net") },
    Scope { name: "wasi", permission: "WASI", channel: "wasi", flag: Some("--allow-wasi") },
    Scope { name: "addon", permission: "Addon", channel: "addon", flag: Some("--allow-addons") },
    Scope { name: "inspector", permission: "Inspector", channel: "inspector", flag: Some("--allow-inspector") },
    Scope { name: "openssl.store", permission: "OpenSSLStore", channel: "openssl-store", flag: Some("--allow-openssl-store") },
    Scope { name: "ffi", permission: "FFI", channel: "ffi", flag: Some("--allow-ffi") },
];
/// The path scopes with the flag granting each its paths.
const PATH_FLAGS: [(&str, &str); 2] = [("fs.read", "--allow-fs-read"), ("fs.write", "--allow-fs-write")];
/// `drop("fs")` drops both path scopes.
const FILESYSTEM: &str = "fs";
const PERMISSION_CHANNEL_PREFIX: &str = "node:permission-model:";

#[derive(Clone)]
enum Grant {
    All,
    Paths(Vec<String>),
}

#[derive(Default)]
struct Model {
    enabled: bool,
    audit: bool,
    grants: std::collections::HashMap<&'static str, Grant>,
    revoked: std::collections::HashSet<&'static str>,
    publishing: bool,
}

thread_local! {
    static MODEL: RefCell<Model> = RefCell::new(Model::default());
}

fn scope(name: &str) -> Option<&'static Scope> {
    SCOPES.iter().find(|scope| scope.name == name)
}

fn path_scope(name: &str) -> bool {
    PATH_FLAGS.iter().any(|(scope, _)| *scope == name)
}

fn model(flags: &[String]) -> Model {
    let mut model = Model { enabled: flags.iter().any(|flag| flag == ENABLE), audit: flags.iter().any(|flag| flag == AUDIT), ..Model::default() };
    for (index, flag) in flags.iter().enumerate() {
        for (scope, option) in PATH_FLAGS {
            let path = match flag.strip_prefix(option) {
                Some(rest) => match rest.strip_prefix('=') {
                    Some(path) => Some(path.to_owned()),
                    None if rest.is_empty() => flags.get(index + 1).cloned(),
                    None => None,
                },
                None => None,
            };
            if let Some(path) = path {
                match model.grants.entry(scope).or_insert_with(|| Grant::Paths(Vec::new())) {
                    Grant::Paths(paths) => paths.push(path),
                    Grant::All => {}
                }
            }
        }
    }
    for scope in &SCOPES {
        if scope.flag.is_some_and(|flag| flags.iter().any(|given| given == flag)) {
            model.grants.insert(scope.name, Grant::All);
        }
    }
    model
}

/// Defines `process.permission` when the model is on, and removes it otherwise.
pub(crate) fn define(c: &mut Context<'_>, process: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(PERMISSION);
    let flags = super::cli_options_shared_vm::option_flags(c);
    let model = model(&flags);
    if !model.enabled && !model.audit {
        c.delete_property_rooted(process, key)?;
        return Ok(());
    }
    MODEL.set(model);
    let api = c.object_rooted()?;
    for (name, operation_name) in [("has", "permissionHas"), ("drop", "permissionDrop")] {
        let function = c.host_function_named(operation(operation_name), name)?;
        c.assign_named_property_rooted(api, name, function)?;
    }
    let object = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        get(c, global, "Object")
    })?;
    let freeze = get(c, object, "freeze")?;
    c.call_rooted(freeze, object, &[api])?;
    c.delete_property_rooted(process, key)?;
    c.define_data_property_rooted(process, key, api, false, true, false)?;
    Ok(())
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

/// A resource reference as the path it names for `scope`: a string as it is; a file URL
/// or byte view only for a path scope (another scope ignores it).
fn reference_path(c: &mut Context<'_>, scope: &str, reference: RootId) -> Result<Option<String>, RootedError> {
    if c.rooted_value(reference).is_some_and(|value| value.is_undefined()) {
        return Ok(None);
    }
    if let Some(text) = c.string_text(reference)? {
        return Ok(Some(text));
    }
    let global = c.global_root()?;
    let url = get(c, global, "URL")?;
    if c.is_object_rooted(reference)? && c.is_callable_rooted(url)? && c.instance_of_rooted(reference, url)? {
        if !path_scope(scope) {
            return Ok(None);
        }
        let protocol = get(c, reference, "protocol")?;
        if c.string_text(protocol)?.as_deref() != Some(FILE_SCHEME) {
            let error = c.type_error_rooted("The URL must use the file: scheme")?;
            super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_URL_SCHEME")?;
            return Err(c.throw(error));
        }
        let undefined = c.undefined();
        let path = super::url_shared_vm::file_url_to_path(c, undefined, &[reference])?;
        return c.string_text(path);
    }
    if c.has_builtin_brand_rooted(reference, quench_runtime::BuiltinBrand::ArrayBufferView)? {
        if !path_scope(scope) {
            return Ok(None);
        }
        let bytes = c.view_bytes_rooted(reference).unwrap_or_default();
        return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
    }
    Err(super::util_shared_vm::argument_type_error(c, reference, "reference", "string")?)
}

fn resolve(c: &mut Context<'_>, path: &str) -> String {
    let cwd = c.host_mut().shared_state().borrow().cwd.path();
    super::path_algorithms::posix::resolve_strings(&[path.to_owned()], &cwd.to_string_lossy())
}

/// Whether the path grant `entry` covers resolved `target`: the entry itself, what lies
/// under it, or with a wildcard whatever starts with the text before it.
fn covers(c: &mut Context<'_>, entry: &str, target: &str) -> bool {
    if entry == ANY_PATH {
        return true;
    }
    let base = resolve(c, entry);
    if let Some(index) = base.find(WILDCARD) {
        let prefix = &base[..index];
        return target == prefix.trim_end_matches('/') || target.starts_with(prefix);
    }
    let directory = if base.ends_with('/') { base.clone() } else { format!("{base}/") };
    target == base || target.starts_with(&directory)
}

fn granted(c: &mut Context<'_>, scope: &'static Scope, target: Option<String>) -> Result<bool, RootedError> {
    let (enabled, audit, revoked, grant) =
        MODEL.with_borrow(|model| (model.enabled, model.audit, model.revoked.contains(scope.name), model.grants.get(scope.name).cloned()));
    if !enabled && !audit {
        return Ok(true);
    }
    if revoked {
        return Ok(false);
    }
    Ok(match (grant, target) {
        (Some(Grant::All), _) => true,
        (Some(Grant::Paths(paths)), None) => paths.iter().any(|path| path == ANY_PATH),
        (paths, Some(target)) if path_scope(scope.name) => {
            let target = resolve(c, &target);
            let process = c.host_mut().shared_state().borrow().process_module;
            let entry = match process {
                Some(process) => {
                    let argv = get(c, process, "argv")?;
                    let entry = get(c, argv, "1")?;
                    c.string_text(entry)?
                }
                None => None,
            };
            let entry_read = scope.name == PATH_FLAGS[0].0 && entry.is_some_and(|entry| resolve(c, &entry) == target);
            entry_read
                || match paths {
                    Some(Grant::Paths(paths)) => paths.iter().any(|path| covers(c, path, &target)),
                    _ => false,
                }
        }
        _ => false,
    })
}

/// Publishes a denied check to `node:permission-model:<channel>`, never recursively.
fn publish(c: &mut Context<'_>, scope: &Scope, message: RootId) -> Result<(), RootedError> {
    if MODEL.with_borrow(|model| model.publishing) {
        return Ok(());
    }
    MODEL.with_borrow_mut(|model| model.publishing = true);
    let published = (|| {
        let channels = super::diagnostics_channel_shared_vm::module(c)?;
        let channel = get(c, channels, "channel")?;
        let name = c.string_rooted(&format!("{PERMISSION_CHANNEL_PREFIX}{}", scope.channel));
        let channel = c.call_rooted(channel, channels, &[name])?;
        let publish = get(c, channel, "publish")?;
        c.call_rooted(publish, channel, &[message])
    })();
    MODEL.with_borrow_mut(|model| model.publishing = false);
    published.map(drop)
}

/// `permission.has(scope[, reference])`.
pub(crate) fn has(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let name = argument(c, args, 0);
    let name = super::util_shared_vm::validate_string(c, name, "scope")?;
    let reference = argument(c, args, 1);
    let target = reference_path(c, &name, reference)?;
    let Some(scope) = scope(&name) else {
        return Ok(c.boolean(false));
    };
    let allowed = granted(c, scope, target.clone())?;
    if !allowed {
        let message = c.object_rooted()?;
        let permission = c.string_rooted(scope.permission);
        c.assign_named_property_rooted(message, "permission", permission)?;
        let resource = match &target {
            Some(target) => c.string_rooted(target),
            None => c.undefined(),
        };
        c.assign_named_property_rooted(message, "resource", resource)?;
        let _ = publish(c, scope, message);
    }
    Ok(c.boolean(allowed))
}

fn publish_drop(c: &mut Context<'_>, scope: &Scope) -> Result<(), RootedError> {
    let message = c.object_rooted()?;
    let permission = c.string_rooted(scope.permission);
    c.assign_named_property_rooted(message, "permission", permission)?;
    let yes = c.boolean(true);
    c.assign_named_property_rooted(message, "drop", yes)?;
    let _ = publish(c, scope, message);
    Ok(())
}

/// `permission.drop(scope[, reference])`: revokes a scope, or one explicit path grant.
pub(crate) fn drop_permission(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let name = argument(c, args, 0);
    let name = super::util_shared_vm::validate_string(c, name, "scope")?;
    let reference = argument(c, args, 1);
    let target = reference_path(c, &name, reference)?;
    if !MODEL.with_borrow(|model| model.enabled || model.audit) {
        let error = c.error_rooted("Cannot drop permissions when permission is not enabled")?;
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_ACCESS_DENIED")?;
        return Err(c.throw(error));
    }
    let scopes: Vec<&'static Scope> = if name == FILESYSTEM {
        if target.is_some() {
            return Ok(undefined);
        }
        PATH_FLAGS.iter().filter_map(|(scope_name, _)| scope(scope_name)).collect()
    } else {
        match scope(&name) {
            Some(scope) => vec![scope],
            None => return Ok(undefined),
        }
    };
    for scope in scopes {
        match (&target, path_scope(scope.name)) {
            (Some(target), true) => {
                let target = resolve(c, target);
                let paths = MODEL.with_borrow(|model| match model.grants.get(scope.name) {
                    Some(Grant::Paths(paths)) => paths.clone(),
                    _ => Vec::new(),
                });
                // Only an exact explicit grant goes: an ancestor's stays, as does a separate
                // grant under the dropped path.
                let mut kept = Vec::with_capacity(paths.len());
                for path in paths {
                    if path == ANY_PATH || resolve(c, &path) != target {
                        kept.push(path);
                    }
                }
                MODEL.with_borrow_mut(|model| {
                    if kept.is_empty() {
                        model.grants.remove(scope.name);
                    } else {
                        model.grants.insert(scope.name, Grant::Paths(kept));
                    }
                });
            }
            _ => MODEL.with_borrow_mut(|model| {
                model.grants.remove(scope.name);
                model.revoked.insert(scope.name);
            }),
        }
        publish_drop(c, scope)?;
    }
    Ok(undefined)
}
