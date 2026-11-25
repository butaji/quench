//! The builtin families an image may link (`NodeHost` capabilities): each wrapper reproduces
//! the arms of `commonjs::build_builtin` for one module family, so an image whose program
//! cannot reach the family names none of its builders.

use crate::host::shared_vm::commonjs::{cached_builtin, BuiltinModule};
use crate::modules::util_shared_vm::named_get as get;
use quench_runtime::{NativeContext, RootId, RootedError};

/// Every family, for the CLI and any host that runs arbitrary source.
pub const ALL: crate::host::BuiltinFamilies = crate::host::BuiltinFamilies {
    tls: Some(tls),
    crypto: Some(crypto),
    http: Some(http),
    http2: Some(http2),
    https: Some(https),
    net: Some(net),
    dns: Some(dns),
    dgram: Some(dgram),
    zlib: Some(zlib),
    child_process: Some(child_process),
    worker_threads: Some(worker_threads),
    vm: Some(vm),
    v8: Some(v8),
    readline: Some(readline),
    repl: Some(repl),
    cluster: Some(cluster),
    perf_hooks: Some(perf_hooks),
    sqlite: Some(sqlite),
};

type Context<'a> = NativeContext<'a>;

pub fn tls(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::tls_shared_vm::build_module(context),
        Some(name) => {
            let process = context
                .host_as::<crate::host::NodeHost>()
                .shared_state()
                .borrow()
                .process_module
                .ok_or_else(|| RootedError::host("shared process module is not initialized"))?;
            crate::modules::tls_shared_vm::deprecated_internal_module(context, process, name)
        }
    }
}

pub fn crypto(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::crypto_node::exports::module(context),
        Some(_) => crate::modules::crypto_node::keys::internal_module(context),
    }
}

pub fn http(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::http_shared_vm::build_module(context),
        Some("_http_common") => crate::modules::http_common_shared_vm::module(context),
        Some(name) => crate::modules::http_shared_vm::internal_module(context, name),
    }
}

pub fn http2(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::http2_shared_vm::build_module(context),
        Some(_) => crate::modules::http2_shared_vm::binding(context),
    }
}

pub fn https(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::https_shared_vm::module(context)
}

pub fn net(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::net_shared_vm::module(context),
        Some(_) => crate::modules::net_internal_shared_vm::module(context),
    }
}

pub fn dns(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::dns_module_shared_vm::module(context),
        Some("promises") => crate::modules::dns_module_shared_vm::promises(context),
        Some("binding") => crate::modules::cares_binding::module(context),
        Some(_) => {
            let dns = cached_builtin(context, BuiltinModule::Dns)?;
            get(context, dns, "promises")
        }
    }
}

pub fn dgram(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::dgram_shared_vm::module(context)
}

pub fn zlib(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::zlib_shared_vm::module(context)
}

pub fn child_process(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::child_process_shared_vm::module(context),
        // The public module's `ChildProcess` and `spawnSync`.
        Some(_) => {
            let public = cached_builtin(context, BuiltinModule::ChildProcess)?;
            let module = context.object_rooted()?;
            for name in ["ChildProcess", "spawnSync"] {
                let value = get(context, public, name)?;
                crate::modules::util_shared_vm::named_put(context, module, name, value)?;
            }
            Ok(module)
        }
    }
}

pub fn worker_threads(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::worker_shared_vm::module(context)
}

pub fn vm(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::vm_shared_vm::module(context)
}

pub fn v8(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::v8_shared_vm::module(context)
}

pub fn readline(context: &mut Context<'_>, internal: Option<&'static str>) -> Result<RootId, RootedError> {
    match internal {
        None => crate::modules::readline_shared_vm::module(context),
        Some(_) => crate::modules::readline_shared_vm::promises_module(context),
    }
}

pub fn repl(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::repl::module(context)
}

pub fn cluster(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::cluster::module(context)
}

pub fn perf_hooks(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::perf_hooks::module(context)
}

pub fn sqlite(context: &mut Context<'_>, _: Option<&'static str>) -> Result<RootId, RootedError> {
    crate::modules::sqlite_shared_vm::module(context)
}
