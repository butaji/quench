//! Native getaddrinfo subset of cares_wrap. Deliberately no ChannelWrap facade.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::sync::mpsc;
#[path = "dns_backend.rs"]
mod backend;
#[path = "dns_service_backend.rs"]
mod service_backend;
#[path = "dns_uv_error.rs"]
mod uv_error;
type Context<'a> = NativeContext<'a, NodeHost>;
const DNS_ORDER_VERBATIM: i32 = 0;
const DNS_ORDER_IPV4_FIRST: i32 = 1;
const DNS_ORDER_IPV6_FIRST: i32 = 2;
enum Completion {
    Addresses(Result<Vec<std::net::IpAddr>, backend::ResolveError>),
    Name(Result<service_backend::NameInfo, backend::ResolveError>),
}
pub(crate) struct Pending {
    request: RootId,
    async_id: u64,
    receiver: mpsc::Receiver<Completion>,
}
#[derive(Default)]
pub(crate) struct State {
    array_factory: Option<RootId>,
    pending: Vec<Pending>,
    pub(crate) default_order: Option<String>,
}
impl State {
    pub(crate) fn has_work(&self) -> bool {
        !self.pending.is_empty()
    }
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)
        .map(|_| ())
}
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let object = c.object_rooted()?;
    let factory = c.host_function(crate::host::shared_vm::operation("dnsAddressArray"))?;
    let factory = c.retain(factory)?;
    c.host_mut().shared_state().borrow_mut().dns.array_factory = Some(factory);
    let request = c.host_constructor(crate::host::shared_vm::operation("dnsRequestConstruct"))?;
    put(c, object, "GetAddrInfoReqWrap", request)?;
    let operation = c.host_function(crate::host::shared_vm::operation("dnsGetAddrInfo"))?;
    put(c, object, "getaddrinfo", operation)?;
    let request = c.host_constructor(crate::host::shared_vm::operation("dnsRequestConstruct"))?;
    put(c, object, "GetNameInfoReqWrap", request)?;
    let operation = c.host_function(crate::host::shared_vm::operation("dnsGetNameInfo"))?;
    put(c, object, "getnameinfo", operation)?;
    for (name, number) in [
        ("AF_INET", libc::AF_INET),
        ("AF_INET6", libc::AF_INET6),
        ("AF_UNSPEC", libc::AF_UNSPEC),
        ("AI_ADDRCONFIG", libc::AI_ADDRCONFIG),
        ("AI_ALL", libc::AI_ALL),
        ("AI_V4MAPPED", libc::AI_V4MAPPED),
        ("DNS_ORDER_VERBATIM", DNS_ORDER_VERBATIM),
        ("DNS_ORDER_IPV4_FIRST", DNS_ORDER_IPV4_FIRST),
        ("DNS_ORDER_IPV6_FIRST", DNS_ORDER_IPV6_FIRST),
    ] {
        let value = c.number(number as f64);
        put(c, object, name, value)?;
    }
    #[cfg(quench_node_cares)]
    super::cares_channel_shared_vm::install(c, object)?;
    Ok(object)
}
pub(crate) fn address_array(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    c.array_rooted(args)
}
pub(crate) fn construct(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // Native constructor receiver is allocated by runtime, preserving subclass prototype.
    let _ = c;
    Ok(receiver)
}
fn integer(c: &Context<'_>, value: RootId) -> Result<i32, RootedError> {
    c.rooted_value(value)
        .and_then(Value::as_number)
        .filter(|n| {
            n.is_finite() && n.fract() == 0.0 && *n >= i32::MIN as f64 && *n <= i32::MAX as f64
        })
        .map(|n| n as i32)
        .ok_or_else(|| RootedError::host("invalid cares_wrap integer argument"))
}
pub(crate) fn getaddrinfo(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 5 {
        return Err(RootedError::host(
            "cares_wrap.getaddrinfo requires five arguments",
        ));
    }
    let hostname = c
        .string_text(args[1])?
        .ok_or_else(|| RootedError::host("invalid cares_wrap hostname"))?;
    // Same IDNA crate already used by URL host policy; invalid names reach native EAI_NONAME.
    let hostname = idna::domain_to_ascii(&hostname).unwrap_or(hostname);
    let family = match integer(c, args[2])? {
        0 => backend::Family::Any,
        4 => backend::Family::V4,
        6 => backend::Family::V6,
        _ => return Err(RootedError::host("invalid cares_wrap family")),
    };
    let hints = integer(c, args[3])?;
    let order = match integer(c, args[4])? {
        DNS_ORDER_IPV4_FIRST => backend::Order::V4First,
        DNS_ORDER_IPV6_FIRST => backend::Order::V6First,
        _ => backend::Order::Verbatim,
    };
    submit(c, args[0], "GETADDRINFOREQWRAP", move || {
        Completion::Addresses(backend::resolve(&hostname, family, hints, order))
    })
}
pub(crate) fn getnameinfo(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 3 {
        return Err(RootedError::host(
            "cares_wrap.getnameinfo requires three arguments",
        ));
    }
    let address = c
        .string_text(args[1])?
        .and_then(|text| text.parse::<std::net::IpAddr>().ok())
        .ok_or_else(|| RootedError::host("cares_wrap.getnameinfo requires a numeric IP address"))?;
    let port = c
        .rooted_value(args[2])
        .and_then(Value::as_number)
        .filter(|number| {
            number.is_finite()
                && number.fract() == 0.0
                && *number >= 0.0
                && *number <= u32::MAX as f64
        })
        .ok_or_else(|| RootedError::host("cares_wrap.getnameinfo requires uint32 port"))?
        as u32 as u16;
    submit(c, args[0], "GETNAMEINFOREQWRAP", move || {
        Completion::Name(service_backend::lookup(address, port))
    })
}
fn submit(
    c: &mut Context<'_>,
    request_object: RootId,
    resource_type: &str,
    work: impl FnOnce() -> Completion + Send + 'static,
) -> Result<RootId, RootedError> {
    let state = c.host_mut().shared_state();
    let async_id = super::async_hooks_shared_vm::create_context(&state);
    if let Err(error) =
        super::async_hooks_shared_vm::emit_init(c, request_object, resource_type, async_id)
    {
        super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
        return Err(error);
    }
    let request = match c.retain(request_object) {
        Ok(request) => request,
        Err(error) => {
            super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
            return Err(error);
        }
    };
    let (reply, receiver) = mpsc::channel();
    let submitted = state.borrow_mut().native_tasks.submit(move || {
        let _ = reply.send(work());
    });
    if let Err(error) = submitted {
        c.release_root(request);
        super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
        return match error.raw_os_error() {
            Some(errno) => Ok(c.number((-errno) as f64)),
            None => Err(RootedError::host(format!(
                "native DNS work submission failed: {error}"
            ))),
        };
    }
    state.borrow_mut().dns.pending.push(Pending {
        request,
        async_id,
        receiver,
    });
    Ok(c.number(0.0))
}
/// Poll phase transfers retained roots to the canonical native callback dispatcher.
pub(crate) fn poll(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    let shared = runtime.host_mut().shared_state();
    let completed = {
        let mut state = shared.borrow_mut();
        let mut ready = None;
        for index in 0..state.dns.pending.len() {
            match state.dns.pending[index].receiver.try_recv() {
                Ok(result) => {
                    ready = Some((state.dns.pending.remove(index), result));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    let pending = state.dns.pending.remove(index);
                    drop(state);
                    super::async_hooks_shared_vm::complete_runtime_context(
                        runtime,
                        pending.async_id,
                    );
                    runtime.release_root(pending.request);
                    return Err("native DNS worker disconnected before completion".to_owned());
                }
            }
        }
        ready
    };
    let Some((pending, outcome)) = completed else {
        return Ok(false);
    };
    let mut temporaries = Vec::new();
    let prepared = (|| {
        let key = runtime.string_rooted("oncomplete");
        temporaries.push(key);
        let callback = runtime.get_property_rooted(pending.request, key)?;
        let args = match outcome {
            Completion::Addresses(Ok(addresses)) => {
                let mut roots = Vec::new();
                for address in addresses {
                    let root = runtime.string_rooted(&address.to_string());
                    temporaries.push(root);
                    roots.push(root);
                }
                let factory = shared
                    .borrow()
                    .dns
                    .array_factory
                    .expect("DNS array factory initialized");
                let undefined = runtime.root(Value::UNDEFINED);
                temporaries.push(undefined);
                let values = match runtime.call_rooted(factory, undefined, &roots) {
                    Ok(values) => values,
                    Err(error) => {
                        runtime.release_root(callback);
                        return Err(error);
                    }
                };
                vec![runtime.root(Value::number(0.0)), values]
            }
            Completion::Name(Ok(info)) => vec![
                runtime.root(Value::number(0.0)),
                runtime.string_rooted(&info.hostname),
                runtime.string_rooted(&info.service),
            ],
            Completion::Addresses(Err(error)) => vec![
                runtime.root(Value::number(uv_error::status(error) as f64)),
                runtime.root(Value::NULL),
            ],
            Completion::Name(Err(error)) => vec![
                runtime.root(Value::number(uv_error::status(error) as f64)),
                runtime.root(Value::NULL),
                runtime.root(Value::NULL),
            ],
        };
        Ok::<_, RootedError>((callback, args))
    })();
    match prepared {
        Ok((callback, args)) => {
            // Retain a separate resource root because receiver and execution own roots independently.
            let resource = runtime.root(
                runtime
                    .rooted_value(pending.request)
                    .expect("retained DNS request"),
            );
            let callback = super::shared_event_loop::SharedCallback {
                execution: Some(super::shared_event_loop::CallbackExecution {
                    lifetime: crate::modules::shared_event_loop::CallbackLifetime::Operation,
                    optional_callback: None,
                    id: pending.async_id,
                    resource,
                }),
                callback,
                receiver: pending.request,
                args,
            };
            for root in temporaries {
                runtime.release_root(root);
            }
            super::process_shared_vm::invoke_async_callback(runtime, program, callback)?;
        }
        Err(error) => {
            for root in temporaries {
                runtime.release_root(root);
            }
            super::async_hooks_shared_vm::complete_runtime_context(runtime, pending.async_id);
            runtime.release_root(pending.request);
            super::process_shared_vm::route_callback_failure(runtime, program, error)?;
        }
    }
    Ok(true)
}
pub(crate) fn cleanup(runtime: &mut quench_runtime::Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let (pending, factory) = {
        let mut state = shared.borrow_mut();
        (
            std::mem::take(&mut state.dns.pending),
            state.dns.array_factory.take(),
        )
    };
    for pending in pending {
        super::async_hooks_shared_vm::complete_runtime_context(runtime, pending.async_id);
        runtime.release_root(pending.request);
    }
    if let Some(factory) = factory {
        runtime.release_root(factory);
    }
}
