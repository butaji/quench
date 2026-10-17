//! `cares_wrap`'s resolver half (src/cares_wrap.cc `ChannelWrap`/`QueryReqWrap`) for hosts
//! without the linked c-ares: queries run the DNS exchange on their own threads, as c-ares
//! runs them beside the event loop. Completions return through a queue the event loop
//! drains, calling each request's `oncomplete` like Node's `ReqWrap::MakeCallback`.
//! `cares_binding` owns the binding object and its address lookups.

mod answers;
mod system;
mod transport;
mod wire;

use crate::host::NodeHost;
use answers::{Answer, Field, Item, QueryKind, Reply, Status, QUERIES};
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use system::Source;
use transport::Config;

type Context<'a> = NativeContext<'a, NodeHost>;

/// The address family a `setServers` entry names.
const FAMILY_IPV4: i32 = 4;
/// ada's IDNA input limit (`max_domain_input_bytes`).
const MAX_DOMAIN_INPUT_BYTES: usize = 16384;

/// The binding's classes; native operations arrive as `native`.
const BINDING_JS: &str = quench_js_check::checked_js!(
    r#"(native, queries) => {
  'use strict';
  function QueryReqWrap() {}
  const channels = new WeakMap();
  const released = new FinalizationRegistry((id) => native.release(id));
  function ChannelWrap(timeout, tries, maxTimeout) {
    const id = native.channel(timeout, tries, maxTimeout);
    channels.set(this, id);
    released.register(this, id);
  }
  const methods = {};
  queries.forEach((name, kind) => {
    methods[name] = { [name](...args) { return native.query(channels.get(this), kind, ...args); } }[name];
  });
  methods.getServers = function getServers(...args) { return native.getServers(channels.get(this), ...args); };
  methods.setServers = function setServers(...args) { return native.setServers(channels.get(this), ...args); };
  methods.setLocalAddress = function setLocalAddress(...args) { return native.setLocalAddress(channels.get(this), ...args); };
  methods.cancel = function cancel(...args) { return native.cancel(channels.get(this), ...args); };
  Object.assign(ChannelWrap.prototype, methods);
  return { QueryReqWrap, ChannelWrap };
}"#
);

struct Completion {
    request: u64,
    outcome: Result<Reply, Status>,
}

type Completions = Arc<Mutex<VecDeque<Completion>>>;

fn push(completions: &Completions, completion: Completion) {
    completions.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_back(completion);
}

struct Pending {
    /// The retained request object whose `oncomplete` receives the outcome.
    request: RootId,
    /// The query's channel.
    channel: u64,
}

/// This VM's in-flight requests and resolver channels.
#[derive(Default)]
pub(crate) struct State {
    next_id: u64,
    pending: HashMap<u64, Pending>,
    channels: HashMap<u64, Config>,
    completions: Completions,
}

impl State {
    fn allocate(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Forget guest handles when the VM heap is replaced; in-flight results are dropped.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

fn state(c: &mut Context<'_>) -> std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>> {
    c.host_mut().shared_state()
}

pub(crate) fn has_work(shared_state: &std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>) -> bool {
    !shared_state.borrow().dns_channels.pending.is_empty()
}

/// Event-loop source: deliver finished queries.
pub(crate) fn poll(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>,
) -> Result<bool, String> {
    let ready = {
        let state = shared_state.borrow();
        !state.dns_channels.pending.is_empty()
            && !state.dns_channels.completions.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty()
    };
    if !ready {
        return Ok(false);
    }
    crate::modules::process_shared_vm::deliver_each(runtime, program, "caresDrain")
}

/// `caresDrain()`: call one finished request's `oncomplete`; `false` when none is ready.
pub(crate) fn drain(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let shared = state(c);
    let (pending, outcome) = loop {
        let next = shared.borrow().dns_channels.completions.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).pop_front();
        let Some(completion) = next else { return Ok(c.boolean(false)) };
        // A cancelled query's late result finds its request already completed.
        if let Some(pending) = shared.borrow_mut().dns_channels.pending.remove(&completion.request) {
            break (pending, completion.outcome);
        }
    };
    let request = pending.request;
    let args = match outcome {
        Ok(reply) => {
            let mut args = vec![c.number(0.0), answer(c, reply.answer)?];
            if let Some(ttls) = reply.ttls {
                let ttls = ttls.into_iter().map(|ttl| c.number(f64::from(ttl))).collect::<Vec<_>>();
                args.push(c.array_rooted(&ttls)?);
            }
            args
        }
        Err(status) => vec![c.string_rooted(status.code())],
    };
    let key = c.string_rooted("oncomplete");
    let result = c.get_property_rooted(request, key).and_then(|callback| c.call_rooted(callback, request, &args));
    c.release_root(request);
    result?;
    Ok(c.boolean(true))
}

fn field(c: &mut Context<'_>, field: Field) -> Result<RootId, RootedError> {
    Ok(match field {
        Field::Text(text) => c.string_rooted(&text),
        Field::Number(number) => c.number(number),
        Field::Undefined => c.undefined(),
        Field::Strings(strings) => {
            let strings = strings.iter().map(|text| c.string_rooted(text)).collect::<Vec<_>>();
            c.array_rooted(&strings)?
        }
        Field::ArrayBuffer(bytes) => {
            let view = c.uint8_array_rooted(&bytes)?;
            let key = c.string_rooted("buffer");
            c.get_property_rooted(view, key)?
        }
    })
}

fn object(c: &mut Context<'_>, properties: answers::Object) -> Result<RootId, RootedError> {
    let object = c.object_rooted()?;
    for (name, value) in properties {
        let key = c.string_rooted(&name);
        let value = field(c, value)?;
        c.define_data_property_rooted(object, key, value, true, true, true)?;
    }
    Ok(object)
}

fn answer(c: &mut Context<'_>, answer: Answer) -> Result<RootId, RootedError> {
    let items = match answer {
        Answer::Object(properties) => return object(c, properties),
        Answer::List(items) => items,
    };
    let array = c.array_rooted(&[])?;
    for (index, item) in items.into_iter().enumerate() {
        let value = match item {
            None => continue,
            Some(Item::Text(text)) => c.string_rooted(&text),
            Some(Item::Strings(strings)) => field(c, Field::Strings(strings))?,
            Some(Item::Object(properties)) => object(c, properties)?,
        };
        let key = c.string_rooted(&index.to_string());
        c.define_data_property_rooted(array, key, value, true, true, true)?;
    }
    Ok(array)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

fn argument(args: &[RootId], index: usize, c: &mut Context<'_>) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn int32(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<i32, RootedError> {
    let value = argument(args, index, c);
    let number = c.to_number(value)?;
    // ECMAScript ToInt32: wrap modulo 2^32.
    Ok(if number.is_finite() { number.trunc().rem_euclid(4_294_967_296.0) as u32 as i32 } else { 0 })
}

fn text(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<String, RootedError> {
    let value = argument(args, index, c);
    c.to_string(value)
}

/// Add the resolver half to `cares_binding`'s `internalBinding('cares_wrap')`.
pub(crate) fn install_channel(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    for (name, operation) in [
        ("canonicalizeIP", "caresCanonicalizeIp"),
        ("convertIpv6StringToBuffer", "caresIpv6Buffer"),
        ("strerror", "caresStrerror"),
    ] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, binding, name, function)?;
    }
    let native = c.object_rooted()?;
    for (name, operation) in [
        ("channel", "caresChannel"),
        ("release", "caresChannelRelease"),
        ("query", "caresQuery"),
        ("getServers", "caresGetServers"),
        ("setServers", "caresSetServers"),
        ("setLocalAddress", "caresSetLocalAddress"),
        ("cancel", "caresCancel"),
    ] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, native, name, function)?;
    }
    let names = QUERIES.iter().map(|(name, _)| c.string_rooted(name)).collect::<Vec<_>>();
    let names = c.array_rooted(&names)?;
    let factory = c.evaluate_script_rooted(BINDING_JS, "node:internal/cares_wrap.js")?;
    let undefined = c.undefined();
    let classes = c.call_rooted(factory, undefined, &[native, names])?;
    for name in ["QueryReqWrap", "ChannelWrap"] {
        let key = c.string_rooted(name);
        let class = c.get_property_rooted(classes, key)?;
        put(c, binding, name, class)?;
    }
    Ok(())
}

/// Node's `ada::idna::to_ascii`: ASCII input lowercases; other input maps through UTS 46;
/// failure is the empty name.
fn to_ascii(name: &str) -> String {
    if name.len() > MAX_DOMAIN_INPUT_BYTES {
        return String::new();
    }
    if name.is_ascii() {
        return name.to_ascii_lowercase();
    }
    idna::domain_to_ascii(name).unwrap_or_default()
}

/// Track `request` until its outcome arrives.
fn register(c: &mut Context<'_>, request: RootId, channel: u64) -> Result<(u64, Completions), RootedError> {
    let request = c.retain(request)?;
    let shared = state(c);
    let mut shared = shared.borrow_mut();
    let id = shared.dns_channels.allocate();
    shared.dns_channels.pending.insert(id, Pending { request, channel });
    Ok((id, shared.dns_channels.completions.clone()))
}

/// `canonicalizeIP(ip)`: libuv's text for a parsed address, otherwise undefined.
pub(crate) fn canonicalize_ip(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let ip = text(c, args, 0)?;
    Ok(match system::parse_ip(&ip) {
        Some(address) => c.string_rooted(&system::ip_text(address)),
        None => c.undefined(),
    })
}

/// `convertIpv6StringToBuffer(ip)`: the address's sixteen bytes as a Buffer.
pub(crate) fn ipv6_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let ip = text(c, args, 0)?;
    let Some(IpAddr::V6(address)) = system::parse_ip(&ip).filter(IpAddr::is_ipv6) else {
        let error = c.error_rooted("Invalid IPv6 address")?;
        return Err(c.throw(error));
    };
    let view = c.uint8_array_rooted(&address.octets())?;
    let key = c.string_rooted("buffer");
    let buffer = c.get_property_rooted(view, key)?;
    let constructor = crate::modules::buffer_shared_vm::fast_buffer(c)?;
    c.construct_rooted(constructor, constructor, &[buffer])
}

/// `strerror(code)`.
pub(crate) fn strerror(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let code = int32(c, args, 0)?;
    Ok(c.string_rooted(answers::strerror(code)))
}

/// `native.channel(timeout, tries, maxTimeout)`: a new channel's id.
pub(crate) fn channel(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let config = Config::new(int32(c, args, 0)?, int32(c, args, 1)?, int32(c, args, 2)?);
    let shared = state(c);
    let mut shared = shared.borrow_mut();
    let id = shared.dns_channels.allocate();
    shared.dns_channels.channels.insert(id, config);
    Ok(c.number(id as f64))
}

fn channel_id(c: &mut Context<'_>, args: &[RootId]) -> Result<u64, RootedError> {
    let value = argument(args, 0, c);
    c.rooted_value(value)
        .and_then(Value::as_number)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("Illegal invocation"))
}

fn with_channel<T>(c: &mut Context<'_>, args: &[RootId], use_config: impl FnOnce(&mut Config) -> T) -> Result<T, RootedError> {
    let id = channel_id(c, args)?;
    let shared = state(c);
    let mut shared = shared.borrow_mut();
    let config = shared.dns_channels.channels.get_mut(&id).ok_or_else(|| RootedError::host("released DNS channel"))?;
    Ok(use_config(config))
}

/// `native.release(id)`: the channel object was collected.
pub(crate) fn channel_release(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = channel_id(c, args)?;
    state(c).borrow_mut().dns_channels.channels.remove(&id);
    Ok(c.undefined())
}

/// `native.query(channel, kind, req, name)`: 0 once sent, or a libuv error code.
pub(crate) fn query(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let channel = channel_id(c, args)?;
    let kind = usize::try_from(int32(c, args, 1)?)
        .ok()
        .and_then(|index| QUERIES.get(index))
        .map(|(_, kind)| *kind)
        .ok_or_else(|| RootedError::host("unknown DNS query"))?;
    let request = argument(args, 2, c);
    let name = text(c, args, 3)?;
    let config = with_channel(c, args, |config| config.clone())?;
    let job: Box<dyn FnOnce() -> Result<Reply, Status> + Send> = if kind == QueryKind::Reverse {
        // `ReverseTraits::Send`: only a literal address can be reversed.
        let Some(address) = system::parse_ip(&name) else {
            return Ok(c.number(f64::from(system::uv_code("EINVAL"))));
        };
        Box::new(move || reverse(&config, address))
    } else {
        let name = to_ascii(&name);
        Box::new(move || resolve(&config, &name, kind))
    };
    let (id, completions) = register(c, request, channel)?;
    std::thread::Builder::new()
        .name("quench-dns".to_owned())
        .spawn(move || push(&completions, Completion { request: id, outcome: job() }))
        .map_err(|error| RootedError::host(error.to_string()))?;
    Ok(c.number(0.0))
}

fn resolve(config: &Config, name: &str, kind: QueryKind) -> Result<Reply, Status> {
    let response = transport::query(config, name, kind.record_type())?;
    if let Some(status) = Status::of_response(&response) {
        return Err(status);
    }
    answers::project(kind, &response)
}

/// `ares_gethostbyaddr`: the configured sources in order; DNS failures fall through.
fn reverse(config: &Config, address: IpAddr) -> Result<Reply, Status> {
    for source in system::lookup_order() {
        match source {
            Source::Hosts => {
                if let Some(names) = system::hosts_reverse(address) {
                    let items = names.into_iter().map(|name| Some(Item::Text(name))).collect();
                    return Ok(Reply { answer: Answer::List(items), ttls: None });
                }
            }
            Source::Dns => {
                if let Ok(reply) = resolve(config, &system::reverse_name(address), QueryKind::Reverse) {
                    return Ok(reply);
                }
            }
        }
    }
    Err(Status::NotFound)
}

/// `getServers()`: `[address, port]` pairs.
pub(crate) fn get_servers(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let servers = with_channel(c, args, |config| config.servers.clone())?;
    let mut pairs = Vec::with_capacity(servers.len());
    for server in servers {
        let address = c.string_rooted(&system::ip_text(server.address));
        let port = c.number(f64::from(server.port));
        pairs.push(c.array_rooted(&[address, port])?);
    }
    c.array_rooted(&pairs)
}

/// `setServers([[family, address, port], ...])`: 0, Node's pending-queries status, or
/// c-ares' bad-string status.
pub(crate) fn set_servers(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = channel_id(c, args)?;
    let busy = state(c).borrow().dns_channels.pending.values().any(|pending| pending.channel == id);
    if busy {
        return Ok(c.number(f64::from(answers::SET_SERVERS_PENDING)));
    }
    let list = argument(args, 1, c);
    let key = c.string_rooted("length");
    let length = c.get_property_rooted(list, key)?;
    let length = c.to_number(length)? as usize;
    let mut servers = Vec::with_capacity(length);
    for index in 0..length {
        let key = c.string_rooted(&index.to_string());
        let entry = c.get_property_rooted(list, key)?;
        let mut parts = [0.0, 0.0];
        let mut address = String::new();
        for (slot, name) in ["0", "1", "2"].into_iter().enumerate() {
            let key = c.string_rooted(name);
            let value = c.get_property_rooted(entry, key)?;
            match slot {
                1 => address = c.to_string(value)?,
                _ => parts[slot / 2] = c.to_number(value)?,
            }
        }
        let [family, port] = parts;
        let parsed = system::parse_ip(&address).filter(|parsed| match family as i32 {
            FAMILY_IPV4 => parsed.is_ipv4(),
            _ => parsed.is_ipv6(),
        });
        let Some(address) = parsed else {
            return Ok(c.number(Status::BadStr as i32 as f64));
        };
        servers.push(transport::Server { address, port: port as u16 });
    }
    with_channel(c, args, |config| config.servers = servers)?;
    Ok(c.number(0.0))
}

fn invalid_value(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => {
            let code = c.string_rooted("ERR_INVALID_ARG_VALUE");
            let key = c.string_rooted("code");
            if let Err(error) = c.set_property_rooted(error, key, code, error) {
                return error;
            }
            c.throw(error)
        }
        Err(error) => error,
    }
}

/// `setLocalAddress(primary[, secondary])`: one address per family; a missing family
/// binds to any address.
pub(crate) fn set_local_address(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let primary = text(c, args, 1)?;
    let Some(primary) = system::parse_ip(&primary) else {
        return Err(invalid_value(c, "Invalid IP address."));
    };
    let secondary = argument(args, 2, c);
    let secondary = if c.rooted_value(secondary).is_some_and(Value::is_undefined) {
        None
    } else {
        let text = c.to_string(secondary)?;
        match system::parse_ip(&text) {
            Some(address) if address.is_ipv4() == primary.is_ipv4() => {
                let message = if primary.is_ipv4() { "Cannot specify two IPv4 addresses." } else { "Cannot specify two IPv6 addresses." };
                return Err(invalid_value(c, message));
            }
            Some(address) => Some(address),
            None => return Err(invalid_value(c, "Invalid IP address.")),
        }
    };
    with_channel(c, args, |config| {
        config.local_v4 = None;
        config.local_v6 = None;
        for address in [Some(primary), secondary].into_iter().flatten() {
            match address {
                IpAddr::V4(address) => config.local_v4 = Some(address),
                IpAddr::V6(address) => config.local_v6 = Some(address),
            }
        }
    })?;
    Ok(c.undefined())
}

/// `cancel()`: every in-flight query on the channel completes with ECANCELLED; their
/// exchanges finish unobserved.
pub(crate) fn cancel(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = channel_id(c, args)?;
    let shared = state(c);
    let shared = shared.borrow();
    let mut requests: Vec<u64> = shared
        .dns_channels
        .pending
        .iter()
        .filter(|(_, pending)| pending.channel == id)
        .map(|(request, _)| *request)
        .collect();
    requests.sort_unstable();
    for request in requests {
        push(&shared.dns_channels.completions, Completion { request, outcome: Err(Status::Cancelled) });
    }
    Ok(c.undefined())
}
