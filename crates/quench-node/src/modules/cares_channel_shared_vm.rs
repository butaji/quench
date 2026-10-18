//! Native c-ares ChannelWrap. JS policy comes from the configured original SDK.
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime, Value};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};
#[path = "cares_channel.rs"]
mod channel;
#[path = "cares_projection.rs"]
mod projection;
use channel::{Channel, Config, QueryId, QueryType, Server};
pub(crate) const VERSION: &str = Channel::expected_version();
type Context<'a> = NativeContext<'a, NodeHost>;
const DNS_ESETSRVPENDING: i32 = -1000; // SDK src/cares_wrap.h owns this native-only status.
const METHODS: &[(&str, QueryType)] = &[
    ("queryA", QueryType::A),
    ("queryAaaa", QueryType::Aaaa),
    ("queryAny", QueryType::Any),
    ("queryCname", QueryType::Cname),
    ("queryMx", QueryType::Mx),
    ("queryNs", QueryType::Ns),
    ("queryPtr", QueryType::Ptr),
    ("queryTxt", QueryType::Txt),
    ("querySrv", QueryType::Srv),
    ("querySoa", QueryType::Soa),
    ("queryNaptr", QueryType::Naptr),
    ("queryCaa", QueryType::Caa),
    ("queryTlsa", QueryType::Tlsa),
    ("getHostByAddr", QueryType::Ptr),
];
struct Pending {
    request: RootId,
    channel: RootId,
    async_id: u64,
}
struct Entry {
    weak: HostWeakRoot,
    channel: Channel,
    async_id: u64,
    pending: BTreeMap<QueryId, Pending>,
}
#[derive(Default)]
pub(crate) struct State {
    channels: Vec<Entry>,
}
impl State {
    pub(crate) fn has_work(&self) -> bool {
        self.channels
            .iter()
            .any(|entry| !entry.pending.is_empty() || entry.channel.has_work())
    }
    pub(crate) fn next_timeout(&self) -> Option<Duration> {
        self.channels
            .iter()
            .filter_map(|entry| entry.channel.next_timeout())
            .min()
    }
}
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn integer(c: &Context<'_>, value: RootId) -> Result<i32, RootedError> {
    c.rooted_value(value)
        .and_then(Value::as_number)
        .filter(|number| {
            number.is_finite()
                && number.fract() == 0.0
                && *number >= i32::MIN as f64
                && *number <= i32::MAX as f64
        })
        .map(|number| number as i32)
        .ok_or_else(|| RootedError::host("invalid c-ares integer argument"))
}
fn invalid_local<T>(c: &mut Context<'_>, message: &str) -> Result<T, RootedError> {
    let error = c.type_error_rooted(message)?;
    super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_ARG_VALUE")?;
    Err(c.throw(error))
}
fn host_error(error: channel::Error) -> RootedError {
    RootedError::host(format!("native c-ares: {error}"))
}
fn index(c: &mut Context<'_>, receiver: RootId) -> Result<usize, RootedError> {
    let shared = c.host_mut().shared_state();
    let handles: Vec<_> = shared
        .borrow()
        .cares
        .channels
        .iter()
        .map(|entry| entry.weak)
        .collect();
    for (index, weak) in handles.into_iter().enumerate() {
        if let Some(object) = c.resolve_host_weak_root(weak) {
            if c.strict_equal_rooted(object, receiver)? {
                return Ok(index);
            }
        }
    }
    let error = c.type_error_rooted("Illegal invocation")?;
    Err(c.throw(error))
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    if Channel::version() != Channel::expected_version() {
        return Err(RootedError::host(
            "native c-ares version does not match configured SDK ABI",
        ));
    }
    let constructor =
        c.host_constructor(crate::host::shared_vm::operation("dnsChannelConstruct"))?;
    let prototype = get(c, constructor, "prototype")?;
    for &(method, _) in METHODS {
        let data = c.string_rooted(method);
        let function =
            c.host_function_with_data(crate::host::shared_vm::operation("dnsChannelQuery"), data)?;
        put(c, prototype, method, function)?;
    }
    for (name, operation) in [
        ("getServers", "dnsChannelGetServers"),
        ("setServers", "dnsChannelSetServers"),
        ("setLocalAddress", "dnsChannelSetLocalAddress"),
        ("cancel", "dnsChannelCancel"),
    ] {
        let function = c.host_function(crate::host::shared_vm::operation(operation))?;
        put(c, prototype, name, function)?;
    }
    put(c, binding, "ChannelWrap", constructor)?;
    let query = c.host_constructor(crate::host::shared_vm::operation("dnsRequestConstruct"))?;
    put(c, binding, "QueryReqWrap", query)?;
    for (name, operation) in [
        ("strerror", "dnsChannelStrerror"),
        ("canonicalizeIP", "dnsCanonicalizeIP"),
        ("convertIpv6StringToBuffer", "dnsIpv6Buffer"),
    ] {
        let function = c.host_function(crate::host::shared_vm::operation(operation))?;
        put(c, binding, name, function)?;
    }
    Ok(())
}
pub(crate) fn construct(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() != 3 {
        return Err(RootedError::host(
            "ChannelWrap requires timeout, tries and maximum timeout",
        ));
    }
    let config = Config {
        timeout_ms: integer(c, args[0])?,
        tries: integer(c, args[1])?,
        max_timeout_ms: integer(c, args[2])?,
    };
    let channel = Channel::new(config).map_err(host_error)?;
    let shared = c.host_mut().shared_state();
    let async_id = super::async_hooks_shared_vm::create_context(&shared);
    if let Err(error) = super::async_hooks_shared_vm::emit_init(c, receiver, "DNSCHANNEL", async_id)
    {
        super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
        return Err(error);
    }
    let weak = match c.host_weak_root(receiver) {
        Ok(weak) => weak,
        Err(error) => {
            super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
            return Err(error);
        }
    };
    shared.borrow_mut().cares.channels.push(Entry {
        weak,
        channel,
        async_id,
        pending: BTreeMap::new(),
    });
    Ok(receiver)
}
pub(crate) fn get_servers(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let servers = shared.borrow().cares.channels[index]
        .channel
        .servers()
        .map_err(host_error)?;
    let mut rows = Vec::new();
    for server in servers {
        let ip = c.string_rooted(&server.address.to_string());
        let port = c.number(server.udp_port as f64);
        rows.push(c.array_rooted(&[ip, port])?);
    }
    c.array_rooted(&rows)
}
pub(crate) fn set_servers(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let array = *args
        .first()
        .ok_or_else(|| RootedError::host("setServers requires server tuples"))?;
    let length = get(c, array, "length")?;
    let length = integer(c, length)?;
    let mut servers = Vec::new();
    for offset in 0..length {
        let key = c.string_rooted(&offset.to_string());
        let row = c.get_property_rooted(array, key)?;
        let ip = get(c, row, "1")?;
        let ip = c
            .string_text(ip)?
            .and_then(|text| text.parse::<IpAddr>().ok())
            .ok_or_else(|| RootedError::host("invalid numeric DNS server address"))?;
        let port = get(c, row, "2")?;
        let port = integer(c, port)?;
        let port = u16::try_from(port).map_err(|_| RootedError::host("invalid DNS server port"))?;
        servers.push(Server {
            address: ip,
            udp_port: port,
            tcp_port: port,
        });
    }
    let index = index(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let entry = &mut state.cares.channels[index];
    let result = entry.channel.set_servers(&servers);
    drop(state);
    match result {
        Ok(()) => Ok(c.number(0.0)),
        Err(channel::Error::PendingQueries) => Ok(c.number(DNS_ESETSRVPENDING as f64)),
        Err(channel::Error::Cares(code)) => Ok(c.number(code as f64)),
        Err(error) => Err(host_error(error)),
    }
}
pub(crate) fn set_local(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mut v4 = Ipv4Addr::UNSPECIFIED;
    let mut v6 = Ipv6Addr::UNSPECIFIED;
    let mut families = Vec::new();
    for value in args.iter().take(2) {
        if c.rooted_value(*value)
            .is_some_and(|value| value.is_undefined())
        {
            continue;
        }
        let ip = match c
            .string_text(*value)?
            .and_then(|text| text.parse::<IpAddr>().ok())
        {
            Some(ip) => ip,
            None => return invalid_local(c, "Invalid IP address."),
        };
        let family = match ip {
            IpAddr::V4(ip) => {
                v4 = ip;
                4
            }
            IpAddr::V6(ip) => {
                v6 = ip;
                6
            }
        };
        if families.contains(&family) {
            return invalid_local(
                c,
                if family == 4 {
                    "Cannot specify two IPv4 addresses."
                } else {
                    "Cannot specify two IPv6 addresses."
                },
            );
        }
        families.push(family);
    }
    let index = index(c, receiver)?;
    let shared = c.host_mut().shared_state();
    shared.borrow_mut().cares.channels[index]
        .channel
        .set_local_addresses(v4, v6)
        .map_err(host_error)?;
    Ok(c.undefined())
}
pub(crate) fn cancel(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let index = index(c, receiver)?;
    let shared = c.host_mut().shared_state();
    shared.borrow_mut().cares.channels[index]
        .channel
        .cancel()
        .map_err(host_error)?;
    Ok(c.undefined())
}
pub(crate) fn strerror(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = *args
        .first()
        .ok_or_else(|| RootedError::host("strerror requires status"))?;
    let code = integer(c, value)?;
    let text = if code == DNS_ESETSRVPENDING {
        "There are pending queries.".to_owned()
    } else {
        Channel::error_string(code)
    };
    Ok(c.string_rooted(&text))
}
pub(crate) fn canonicalize(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(value) = args.first() else {
        return Ok(c.undefined());
    };
    let value = c.to_string(*value)?;
    Ok(match value.parse::<IpAddr>() {
        Ok(address) => c.string_rooted(&address.to_string()),
        Err(_) => c.undefined(),
    })
}
pub(crate) fn ipv6_buffer(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = *args
        .first()
        .ok_or_else(|| RootedError::host("IPv6 conversion requires address"))?;
    let value = c.to_string(value)?;
    let ip = match value.parse::<Ipv6Addr>() {
        Ok(ip) => ip,
        Err(_) => {
            let error = c.error_rooted("Invalid IPv6 address")?;
            return Err(c.throw(error));
        }
    };
    super::buffer_shared_vm::from_bytes(c, &ip.octets())
}
pub(crate) fn query(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args.len() < 2 {
        return Err(RootedError::host("DNS query requires request and name"));
    }
    let operation = c.host_function_data()?;
    let method = c
        .string_text(operation)?
        .ok_or_else(|| RootedError::host("missing DNS query operation"))?;
    let kind = METHODS
        .iter()
        .find(|&&(name, _)| name == method)
        .map(|row| row.1)
        .ok_or_else(|| RootedError::host("unregistered DNS record operation"))?;
    let name = c
        .string_text(args[1])?
        .ok_or_else(|| RootedError::host("DNS query name must be string"))?;
    let reverse_address = if method == "getHostByAddr" {
        match name.parse::<IpAddr>() {
            Ok(address) => Some(address),
            Err(_) => return Ok(c.number((-libc::EINVAL) as f64)),
        }
    } else {
        None
    };
    let name = if reverse_address.is_none() {
        idna::domain_to_ascii(&name).unwrap_or(name)
    } else {
        name
    };
    let shared = c.host_mut().shared_state();
    let async_id = super::async_hooks_shared_vm::create_context(&shared);
    if let Err(error) = super::async_hooks_shared_vm::emit_init(c, args[0], "QUERYWRAP", async_id) {
        super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
        return Err(error);
    }
    let request = match c.retain(args[0]) {
        Ok(root) => root,
        Err(error) => {
            super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
            return Err(error);
        }
    };
    let channel_root = match c.retain(receiver) {
        Ok(root) => root,
        Err(error) => {
            c.release_root(request);
            super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
            return Err(error);
        }
    };
    let index = match index(c, receiver) {
        Ok(index) => index,
        Err(error) => {
            c.release_root(request);
            c.release_root(channel_root);
            super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
            return Err(error);
        }
    };
    let result = {
        let mut state = shared.borrow_mut();
        let channel = &mut state.cares.channels[index].channel;
        match reverse_address {
            Some(address) => channel.reverse(address),
            None => channel.query(&name, kind),
        }
    };
    match result {
        Ok(id) => {
            shared.borrow_mut().cares.channels[index].pending.insert(
                id,
                Pending {
                    request,
                    channel: channel_root,
                    async_id,
                },
            );
            Ok(c.number(0.0))
        }
        Err(error) => {
            c.release_root(request);
            c.release_root(channel_root);
            super::async_hooks_shared_vm::complete_callback_context(c, async_id)?;
            Err(host_error(error))
        }
    }
}
// The native immediate relay owns the actual channel until the callback returns,
// and resolves the request's mutable oncomplete slot at invocation time.
pub(crate) fn callback_factory(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let channel = *args
        .first()
        .ok_or_else(|| RootedError::host("DNS callback requires channel owner"))?;
    c.host_function_with_data(crate::host::shared_vm::operation("dnsRrInvoke"), channel)
}
pub(crate) fn invoke(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let _channel = c.host_function_data()?;
    let callback = get(c, receiver, "oncomplete")?;
    c.call_rooted(callback, receiver, args)
}
// Protected native factories materialize the one Rust schema without mutable guest constructors.
pub(crate) fn array(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    c.array_rooted(args)
}
pub(crate) fn object(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let object = c.object_rooted()?;
    for pair in args.chunks_exact(2) {
        c.define_data_property_rooted(object, pair[0], pair[1], true, true, true)?;
    }
    Ok(object)
}
pub(crate) fn binary(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let bytes = args
        .iter()
        .map(|root| integer(c, *root).map(|value| value as u8))
        .collect::<Result<Vec<_>, _>>()?;
    c.with_startup_intrinsics(|c| {
        let array = c.uint8_array_rooted(&bytes)?;
        get(c, array, "buffer")
    })
}
fn materialize(
    runtime: &mut Runtime<NodeHost>,
    value: projection::Js,
    roots: &mut Vec<RootId>,
) -> Result<RootId, RootedError> {
    use projection::Js;
    let root = match value {
        Js::Undefined => runtime.root(Value::UNDEFINED),
        Js::Number(number) => runtime.root(Value::number(number)),
        Js::Text(bytes) => {
            runtime.string_rooted(&bytes.into_iter().map(char::from).collect::<String>())
        }
        value => {
            let (operation, values) = match value {
                Js::Array(values) => ("dnsRrArray", values),
                Js::Object(fields) => {
                    let mut values = Vec::new();
                    for (key, value) in fields {
                        values.push(Js::Text(key.into_bytes()));
                        values.push(value);
                    }
                    ("dnsRrObject", values)
                }
                Js::Binary(bytes) => (
                    "dnsRrBinary",
                    bytes
                        .into_iter()
                        .map(|byte| Js::Number(byte as f64))
                        .collect(),
                ),
                _ => unreachable!(),
            };
            let args = values
                .into_iter()
                .map(|value| materialize(runtime, value, roots))
                .collect::<Result<Vec<_>, _>>()?;
            let function = runtime.host_function(crate::host::shared_vm::operation(operation))?;
            roots.push(function);
            let undefined = runtime.root(Value::UNDEFINED);
            roots.push(undefined);
            runtime.call_rooted(function, undefined, &args)?
        }
    };
    roots.push(root);
    Ok(root)
}
pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    let shared = runtime.host_mut().shared_state();
    let dead: Vec<_> = shared
        .borrow()
        .cares
        .channels
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (!runtime.host_weak_root_is_live(entry.weak)).then_some(index))
        .collect();
    for index in dead.into_iter().rev() {
        let entry = shared.borrow_mut().cares.channels.remove(index);
        runtime.release_host_weak_root(entry.weak);
        super::async_hooks_shared_vm::complete_runtime_context(runtime, entry.async_id);
        drop(entry);
    }
    let completed = {
        let mut state = shared.borrow_mut();
        let mut completed = None;
        for entry in &mut state.cares.channels {
            entry
                .channel
                .poll_ready()
                .map_err(|error| format!("native c-ares readiness: {error}"))?;
            if let Some(result) = entry.channel.take_completion() {
                let pending = entry
                    .pending
                    .remove(&result.id)
                    .ok_or_else(|| "c-ares completion has no retained request".to_owned())?;
                completed = Some((pending, result));
                break;
            }
        }
        completed
    };
    let Some((pending, result)) = completed else {
        return Ok(false);
    };
    let mut temporaries = Vec::new();
    let prepared = (|| {
        let factory =
            runtime.host_function(crate::host::shared_vm::operation("dnsRrCallbackFactory"))?;
        temporaries.push(factory);
        let undefined = runtime.root(Value::UNDEFINED);
        temporaries.push(undefined);
        let callback = runtime.call_rooted(factory, undefined, &[pending.channel])?;
        temporaries.push(callback);
        let args = match result.result {
            Ok(records) => projection::arguments(result.query_type, records),
            Err(channel::Failure::Cares { code, .. }) => vec![projection::Js::Text(
                channel::status_code(code).as_bytes().to_vec(),
            )],
            Err(error) => {
                return Err(RootedError::host(format!(
                    "native DNS record projection: {error:?}"
                )));
            }
        };
        let args = args
            .into_iter()
            .map(|value| materialize(runtime, value, &mut temporaries))
            .collect::<Result<Vec<_>, _>>()?;
        let queue_id = shared
            .borrow_mut()
            .scheduler
            .reserve_shared_immediate_id()
            .ok_or_else(|| RootedError::host("DNS native immediate identifier space exhausted"))?;
        Ok::<_, RootedError>((queue_id, callback, args))
    })();
    match prepared {
        Ok((queue_id, callback, args)) => {
            temporaries.retain(|root| *root != callback && !args.contains(root));
            for root in temporaries {
                runtime.release_root(root);
            }
            let resource = runtime.root(
                runtime
                    .rooted_value(pending.request)
                    .expect("retained DNS query resource"),
            );
            let callback = super::shared_event_loop::SharedCallback {
                execution: Some(super::shared_event_loop::CallbackExecution {
                    lifetime: super::shared_event_loop::CallbackLifetime::Operation,
                    optional_callback: None,
                    id: pending.async_id,
                    resource,
                }),
                callback,
                receiver: pending.request,
                args,
            };
            shared.borrow_mut().scheduler.queue_shared_immediate(
                queue_id,
                super::shared_event_loop::SharedImmediateOwner::Host,
                callback,
            );
            runtime.release_root(pending.channel);
        }
        Err(error) => {
            for root in temporaries {
                runtime.release_root(root);
            }
            super::async_hooks_shared_vm::complete_runtime_context(runtime, pending.async_id);
            runtime.release_root(pending.request);
            runtime.release_root(pending.channel);
            super::process_shared_vm::route_callback_failure(runtime, program, error)?;
        }
    }
    Ok(true)
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().cares.channels);
    for mut entry in entries {
        entry.channel.close();
        for (_, pending) in entry.pending {
            super::async_hooks_shared_vm::complete_runtime_context(runtime, pending.async_id);
            runtime.release_root(pending.request);
            runtime.release_root(pending.channel);
        }
        super::async_hooks_shared_vm::complete_runtime_context(runtime, entry.async_id);
        runtime.release_host_weak_root(entry.weak);
    }
}
