//! Shared native AsyncWrap base. Views come from actual wrapper/IO owners;
//! uninitialized request shells and detached wrappers own no async resource.
use super::event_constructor_shared_vm::get;
use crate::host::{NodeHost, SharedNodeState};
use quench_runtime::{NativeContext, RootId, RootedError, Runtime};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
type Context<'a> = NativeContext<'a, NodeHost>;
const UNINITIALIZED_ASYNC_ID: i64 = -1; // AsyncWrap::kInvalidAsyncId.
                                        // Node src/async_wrap.h NODE_ASYNC_PROVIDER_TYPES, before crypto providers.
const PROVIDER_TYPES: &[&str] = &[
    "NONE",
    "DIRHANDLE",
    "DNSCHANNEL",
    "ELDHISTOGRAM",
    "FILEHANDLE",
    "FILEHANDLECLOSEREQ",
    "BLOBREADER",
    "FSEVENTWRAP",
    "FSREQCALLBACK",
    "FSREQPROMISE",
    "GETADDRINFOREQWRAP",
    "GETNAMEINFOREQWRAP",
    "HEAPSNAPSHOT",
    "HTTP2SESSION",
    "HTTP2STREAM",
    "HTTP2PING",
    "HTTP2SETTINGS",
    "HTTPINCOMINGMESSAGE",
    "HTTPCLIENTREQUEST",
    "LOCKS",
    "DTLS_ENDPOINT",
    "DTLS_SESSION",
    "JSSTREAM",
    "MESSAGEPORT",
    "PIPECONNECTWRAP",
    "PIPESERVERWRAP",
    "PIPEWRAP",
    "PROCESSWRAP",
    "PROMISE",
    "QUERYWRAP",
    "QUIC_ENDPOINT",
    "QUIC_LOGSTREAM",
    "QUIC_SESSION",
    "QUIC_STREAM",
    "QUIC_UDP",
    "SHUTDOWNWRAP",
    "SIGNALWRAP",
    "STATWATCHER",
    "STREAMPIPE",
    "TCPCONNECTWRAP",
    "TCPSERVERWRAP",
    "TCPWRAP",
    "TTYWRAP",
    "UDPSENDWRAP",
    "UDPWRAP",
    "SIGINTWATCHDOG",
    "WORKER",
    "WORKERCPUPROFILE",
    "WORKERCPUUSAGE",
    "WORKERHEAPPROFILE",
    "WORKERHEAPSNAPSHOT",
    "WORKERHEAPSTATISTICS",
    "WRITEWRAP",
    "ZLIB",
];

#[derive(Clone, Copy)]
pub(crate) enum Provider {
    Tcp,
    TcpServer,
    Udp,
    Send,
    Pipe,
    Write,
    Shutdown,
    TcpConnect,
}
impl Provider {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Tcp => "TCPWRAP",
            Self::TcpServer => "TCPSERVERWRAP",
            Self::Udp => "UDPWRAP",
            Self::Send => "UDPSENDWRAP",
            Self::Pipe => "PIPEWRAP",
            Self::Write => "WRITEWRAP",
            Self::Shutdown => "SHUTDOWNWRAP",
            Self::TcpConnect => "TCPCONNECTWRAP",
        }
    }
    fn code(self) -> usize {
        PROVIDER_TYPES
            .iter()
            .position(|name| *name == self.name())
            .expect("native provider is in Node provider ABI")
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Resource {
    pub(crate) id: u64,
    pub(crate) provider: Provider,
}
pub(crate) enum View {
    NotNative,
    Detached,
    Active(Resource),
}
#[derive(Default)]
pub(crate) struct State {
    tcp: HashMap<u64, HandleResource>,
}
struct HandleResource {
    resource: Resource,
    owner: RootId,
}
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
/// Native constructor owner data retains the original prototype, independently
/// of guest writes to public constructor properties.
pub(crate) fn prototype(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let owner =
        crate::host::shared_vm::commonjs::cached_native_binding(c, "\0native:AsyncWrap", |c| {
            let owner = c.object_rooted()?;
            let constructor = c.host_constructor_named(
                crate::host::shared_vm::operation("nativeAsyncWrapConstruct"),
                "AsyncWrap",
            )?;
            let prototype = get(c, constructor, "prototype")?;
            for (name, op) in [
                ("getAsyncId", "nativeAsyncWrapId"),
                ("getProviderType", "nativeAsyncWrapProvider"),
                ("asyncReset", "nativeAsyncWrapReset"),
            ] {
                let method = c.host_function_named(crate::host::shared_vm::operation(op), name)?;
                put(c, prototype, name, method)?;
            }
            put(c, owner, "constructor", constructor)?;
            put(c, owner, "prototype", prototype)?;
            Ok(owner)
        })?;
    get(c, owner, "prototype")
}
pub(crate) fn abstract_constructor(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(if c.new_target_rooted().is_some() {
        receiver
    } else {
        c.undefined()
    })
}
pub(crate) fn require_construct(c: &mut Context<'_>) -> Result<(), RootedError> {
    if c.new_target_rooted().is_some() {
        Ok(())
    } else {
        let error = c.type_error_rooted("Native constructor requires new")?;
        Err(c.throw(error))
    }
}
pub(crate) fn view(c: &mut Context<'_>, receiver: RootId) -> Result<View, RootedError> {
    match super::pipe_binding::resource_view(c, receiver)? {
        View::NotNative => {}
        other => return Ok(other),
    }
    match super::udp_binding::resource_view(c, receiver)? {
        View::NotNative => {}
        other => return Ok(other),
    }
    match super::stream_binding::resource_view(c, receiver)? {
        View::NotNative => {}
        other => return Ok(other),
    }
    if let Some(endpoint) = super::net_reference_shared_vm::optional_handle_id(c, receiver)? {
        if let Some(resource) = tcp_resource(&c.host_mut().shared_state(), endpoint) {
            return Ok(View::Active(resource));
        }
        return Ok(View::Detached);
    }
    Ok(View::NotNative)
}
pub(crate) fn get_id(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = match view(c, receiver)? {
        View::Active(resource) => resource.id as f64,
        _ => UNINITIALIZED_ASYNC_ID as f64,
    };
    Ok(c.number(id))
}
pub(crate) fn get_provider(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let provider = match view(c, receiver)? {
        View::Active(resource) => resource.provider.code(),
        _ => PROVIDER_TYPES
            .iter()
            .position(|name| *name == "NONE")
            .expect("NONE provider ABI"),
    };
    Ok(c.number(provider as f64))
}

/// HTTP Agent reuses a native socket handle under a fresh async identity.
pub(crate) fn async_reset(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(endpoint) = super::net_reference_shared_vm::optional_handle_id(c, receiver)? else {
        return Ok(c.undefined());
    };
    let Some(old) = tcp_resource(&c.host_mut().shared_state(), endpoint) else {
        return Ok(c.undefined());
    };
    super::async_hooks_shared_vm::complete_callback_context(c, old.id)?;
    let shared = c.host_mut().shared_state();
    let id = super::async_hooks_shared_vm::create_context(&shared);
    if let Some(handle) = shared.borrow_mut().native_async_wrap.tcp.get_mut(&endpoint) {
        handle.resource.id = id;
    }
    let resource = args.first().copied().unwrap_or(receiver);
    super::async_hooks_shared_vm::emit_init(c, resource, old.provider.name(), id)?;
    Ok(receiver)
}
pub(crate) fn initialize_tcp(
    c: &mut Context<'_>,
    receiver: RootId,
    endpoint: u64,
    server: bool,
) -> Result<(), RootedError> {
    let shared = c.host_mut().shared_state();
    let id = super::async_hooks_shared_vm::create_context(&shared);
    let provider = if server {
        Provider::TcpServer
    } else {
        Provider::Tcp
    };
    let owner = c.retain(receiver)?;
    shared.borrow_mut().native_async_wrap.tcp.insert(
        endpoint,
        HandleResource {
            resource: Resource { id, provider },
            owner,
        },
    );
    super::async_hooks_shared_vm::emit_init(c, receiver, provider.name(), id)
}
pub(crate) fn tcp_resource(
    shared: &Rc<RefCell<SharedNodeState>>,
    endpoint: u64,
) -> Option<Resource> {
    shared
        .borrow()
        .native_async_wrap
        .tcp
        .get(&endpoint)
        .map(|handle| handle.resource)
}
/// The callback lease is the last native request/close owner. Detach only after
/// guest callback+after hooks, including all throwing paths.
pub(crate) fn detach(runtime: &mut Runtime<NodeHost>, id: u64) {
    super::pipe_binding::detach(runtime, id);
    let shared = runtime.host_mut().shared_state();
    let endpoints = shared
        .borrow()
        .native_async_wrap
        .tcp
        .iter()
        .filter_map(|(endpoint, resource)| (resource.resource.id == id).then_some(*endpoint))
        .collect::<Vec<_>>();
    for endpoint in endpoints {
        let handle = shared.borrow_mut().native_async_wrap.tcp.remove(&endpoint);
        if let Some(handle) = handle {
            runtime.release_root(handle.owner);
        }
        let socket = shared.borrow_mut().net_sockets.remove(&endpoint);
        if let Some(socket) = socket {
            runtime.release_root(socket.root);
        }
    }
    super::udp_binding::detach_resource(&shared, id);
    super::stream_binding::detach_resource(&shared, id);
}
pub(crate) fn collect_tcp(runtime: &mut Runtime<NodeHost>, endpoint: u64) {
    let shared = runtime.host_mut().shared_state();
    let resource = shared.borrow_mut().native_async_wrap.tcp.remove(&endpoint);
    if let Some(handle) = resource {
        runtime.release_root(handle.owner);
        super::async_hooks_shared_vm::complete_runtime_context(runtime, handle.resource.id);
    }
}

/// Final environment cancellation releases native ownership without guest hooks.
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let handles = std::mem::take(&mut shared.borrow_mut().native_async_wrap.tcp);
    for (_, handle) in handles {
        runtime.release_root(handle.owner);
        super::async_hooks_shared_vm::clear_context(runtime, &shared, handle.resource.id);
    }
    let callbacks = std::mem::take(&mut shared.borrow_mut().net_handle_close_callbacks);
    for (_, root) in callbacks {
        runtime.release_root(root);
    }
}
