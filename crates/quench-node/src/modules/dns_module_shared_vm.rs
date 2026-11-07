//! `dns` and `dns/promises` (lib/dns.js, lib/internal/dns/*) over `cares_wrap`: address
//! lookups through `getaddrinfo`/`getnameinfo` request wraps, record queries through a
//! Resolver's `ChannelWrap`. The callback and promise surfaces share one Resolver base and
//! one default channel; only how a completion is delivered differs.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

/// Node's `IANA_DNS_PORT`: a server on it is listed without its port.
const IANA_DNS_PORT: f64 = 53.0;
/// A Resolver option, as `new ChannelWrap(timeout, tries, maxTimeout)` takes it in order:
/// its default and the integer bounds its validator (`validateInt32`/`validateUint32`)
/// checks.
struct ChannelOption {
    name: &'static str,
    default: f64,
    minimum: f64,
    maximum: f64,
}

const MAX_INT32: f64 = i32::MAX as f64;
const CHANNEL_OPTIONS: [ChannelOption; 3] = [
    ChannelOption { name: "timeout", default: -1.0, minimum: -1.0, maximum: MAX_INT32 },
    ChannelOption { name: "tries", default: 4.0, minimum: 1.0, maximum: MAX_INT32 },
    ChannelOption { name: "maxTimeout", default: 0.0, minimum: 0.0, maximum: u32::MAX as f64 },
];
/// The address families `lookup` accepts.
const FAMILIES: [f64; 3] = [0.0, 4.0, 6.0];
const IPV6: f64 = 6.0;
const IPV4: f64 = 4.0;
/// libuv's EAI_NODATA and EAI_NONAME, which Node reports as ENOTFOUND.
const UV_EAI_NODATA: f64 = -3007.0;
const UV_EAI_NONAME: f64 = -3008.0;

/// The result orders, by their option spelling and `cares_wrap` constant.
const ORDERS: [(&str, &str); 3] = [
    ("verbatim", "DNS_ORDER_VERBATIM"),
    ("ipv4first", "DNS_ORDER_IPV4_FIRST"),
    ("ipv6first", "DNS_ORDER_IPV6_FIRST"),
];
const ORDER_LIST: &str = "'verbatim', 'ipv4first', 'ipv6first'";
const FAMILY_LIST: &str = "0, 4, 6";

/// Resolver methods: the public name and the `ChannelWrap` query it issues, in Node's order.
const QUERIES: [(&str, &str, &str); 14] = [
    ("resolveAny", "queryAny", "ANY"),
    ("resolve4", "queryA", "A"),
    ("resolve6", "queryAaaa", "AAAA"),
    ("resolveCaa", "queryCaa", "CAA"),
    ("resolveCname", "queryCname", "CNAME"),
    ("resolveMx", "queryMx", "MX"),
    ("resolveNs", "queryNs", "NS"),
    ("resolveTlsa", "queryTlsa", "TLSA"),
    ("resolveTxt", "queryTxt", "TXT"),
    ("resolveSrv", "querySrv", "SRV"),
    ("resolvePtr", "queryPtr", "PTR"),
    ("resolveNaptr", "queryNaptr", "NAPTR"),
    ("resolveSoa", "querySoa", "SOA"),
    ("reverse", "getHostByAddr", ""),
];

/// The module-level names bound to the default resolver, in export order.
const DEFAULT_RESOLVER_METHODS: [&str; 16] = [
    "getServers",
    "resolve",
    "resolve4",
    "resolve6",
    "resolveAny",
    "resolveCaa",
    "resolveCname",
    "resolveMx",
    "resolveNaptr",
    "resolveNs",
    "resolvePtr",
    "resolveSoa",
    "resolveSrv",
    "resolveTlsa",
    "resolveTxt",
    "reverse",
];

/// c-ares' error codes, exported as `dns.NODATA = 'ENODATA'` and so on.
const ERROR_CODES: [&str; 24] = [
    "NODATA",
    "FORMERR",
    "SERVFAIL",
    "NOTFOUND",
    "NOTIMP",
    "REFUSED",
    "BADQUERY",
    "BADNAME",
    "BADFAMILY",
    "BADRESP",
    "CONNREFUSED",
    "TIMEOUT",
    "EOF",
    "FILE",
    "NOMEM",
    "DESTRUCTION",
    "BADSTR",
    "BADFLAGS",
    "NONAME",
    "BADHINTS",
    "NOTINITIALIZED",
    "LOADIPHLPAPI",
    "ADDRGETNETWORKPARAMS",
    "CANCELLED",
];

/// How a request's completion is delivered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Callback,
    Promise,
}

impl Delivery {
    fn tag(self) -> bool {
        self == Self::Promise
    }
    fn of(promise: bool) -> Self {
        if promise { Self::Promise } else { Self::Callback }
    }
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)?;
    Ok(())
}

fn hide(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, false, true)?;
    Ok(())
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined() || value.is_null())
}

fn number(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

fn call_method(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, name)?;
    c.call_rooted(function, object, args)
}

fn method(c: &mut Context<'_>, target: RootId, name: &str, operation: &str, data: RootId) -> Result<(), RootedError> {
    let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, function, true, false, true)?;
    Ok(())
}

fn validate_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_callable_rooted(value)? {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error(c, value, name, "function")?)
}

fn validate_boolean(c: &mut Context<'_>, value: RootId, name: &str) -> Result<bool, RootedError> {
    match c.rooted_value(value).and_then(|value| value.as_bool()) {
        Some(flag) => Ok(flag),
        None => Err(super::util_shared_vm::argument_type_error(c, value, name, "boolean")?),
    }
}

/// `isIP(text)`: 4, 6 or 0.
fn ip_family(text: &str) -> f64 {
    match super::net_shared_vm::parse_ip(text) {
        Some(std::net::IpAddr::V4(_)) => IPV4,
        Some(std::net::IpAddr::V6(_)) => IPV6,
        None => 0.0,
    }
}

/// The module's fixed members.
struct Intrinsics {
    holder: RootId,
    binding: RootId,
}

fn intrinsics(c: &mut Context<'_>) -> Result<Intrinsics, RootedError> {
    let holder = crate::host::shared_vm::commonjs::cached_native_binding(c, "\0dns:intrinsics", build)?;
    let binding = get(c, holder, "binding")?;
    Ok(Intrinsics { holder, binding })
}

fn intrinsic(c: &mut Context<'_>, name: &str) -> Outcome {
    let holder = intrinsics(c)?.holder;
    get(c, holder, name)
}

fn build(c: &mut Context<'_>) -> Outcome {
    let holder = c.null_object_rooted()?;
    let binding = crate::host::shared_vm::commonjs::cares_binding(c)?;
    put(c, holder, "binding", binding)?;
    let order = c.string_rooted(ORDERS[0].0);
    put(c, holder, "order", order)?;
    let warned = c.boolean(false);
    put(c, holder, "warned", warned)?;

    let base = c.host_constructor_named(crate::host::shared_vm::operation!("dnsResolverBase"), "ResolverBase")?;
    let prototype = get(c, base, "prototype")?;
    let undefined = c.undefined();
    for (name, operation) in [
        ("cancel", "dnsResolverCancel"),
        ("getServers", "dnsResolverGetServers"),
        ("setServers", "dnsResolverSetServers"),
        ("setLocalAddress", "dnsResolverSetLocalAddress"),
    ] {
        method(c, prototype, name, operation, undefined)?;
    }
    put(c, holder, "ResolverBase", base)?;
    for delivery in [Delivery::Callback, Delivery::Promise] {
        let promise = c.boolean(delivery.tag());
        let resolver = c.host_constructor_named_with_data(crate::host::shared_vm::operation!("dnsResolverConstruct"), "Resolver", promise)?;
        c.set_prototype_rooted(resolver, base)?;
        let resolver_prototype = get(c, resolver, "prototype")?;
        c.set_prototype_rooted(resolver_prototype, prototype)?;
        for (name, binding_name, _) in QUERIES {
            let binding_name_root = c.string_rooted(binding_name);
            let data = c.array_rooted(&[promise, binding_name_root])?;
            let function = c.host_function_named_with_data(crate::host::shared_vm::operation!("dnsResolverQuery"), binding_name, data)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(resolver_prototype, key, function, true, false, true)?;
        }
        method(c, resolver_prototype, "resolve", "dnsResolverResolve", promise)?;
        let instance = c.construct_rooted(resolver, resolver, &[])?;
        let (class_name, default_name) = match delivery {
            Delivery::Callback => ("Resolver", "defaultResolver"),
            Delivery::Promise => ("PromisesResolver", "promisesDefaultResolver"),
        };
        put(c, holder, class_name, resolver)?;
        put(c, holder, default_name, instance)?;
    }
    let callback_default = get(c, holder, "defaultResolver")?;
    let handle = get(c, callback_default, "_handle")?;
    let promises_default = get(c, holder, "promisesDefaultResolver")?;
    put(c, promises_default, "_handle", handle)?;
    Ok(holder)
}

/// Node's `DNSException(code, syscall, hostname)`.
fn dns_exception(c: &mut Context<'_>, code: RootId, syscall: &str, hostname: Option<RootId>) -> Outcome {
    let (errno, code) = match number(c, code) {
        Some(errno) => {
            let name = if errno == UV_EAI_NODATA || errno == UV_EAI_NONAME {
                "ENOTFOUND".to_owned()
            } else {
                super::util_system_error_shared_vm::system_error_name(errno)
            };
            (Some(errno), name)
        }
        None => (None, c.to_string(code)?),
    };
    let host = match hostname {
        Some(hostname) if c.truthy_rooted(hostname)? => Some(hostname),
        _ => None,
    };
    let message = match host {
        Some(host) => format!("{syscall} {code} {}", c.to_string(host)?),
        None => format!("{syscall} {code}"),
    };
    let error = c.error_rooted(&message)?;
    let errno = match errno {
        Some(errno) => c.number(errno),
        None => c.undefined(),
    };
    put(c, error, "errno", errno)?;
    let code = c.string_rooted(&code);
    put(c, error, "code", code)?;
    let syscall = c.string_rooted(syscall);
    put(c, error, "syscall", syscall)?;
    if let Some(host) = host {
        put(c, error, "hostname", host)?;
    }
    Ok(error)
}

/// The resolver channel options `new Resolver(options)` accepts.
fn channel(c: &mut Context<'_>, options: RootId) -> Outcome {
    let mut settings = CHANNEL_OPTIONS.map(|option| option.default);
    if c.is_object_rooted(options)? {
        for (setting, option) in settings.iter_mut().zip(CHANNEL_OPTIONS) {
            let value = get(c, options, option.name)?;
            if !is_undefined(c, value) {
                let maximum = c.number(option.maximum);
                *setting = super::util_shared_vm::validate_integer(c, value, &format!("options.{}", option.name), option.minimum, maximum)?;
            }
        }
    } else if !is_undefined(c, options) {
        return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let binding = crate::host::shared_vm::commonjs::cares_binding(c)?;
    let channel = get(c, binding, "ChannelWrap")?;
    let arguments = settings.map(|setting| c.number(setting));
    c.construct_rooted(channel, channel, &arguments)
}

pub(crate) fn resolver_base(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    let handle = channel(c, options)?;
    put(c, this, "_handle", handle)?;
    Ok(this)
}

pub(crate) fn resolver_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Class constructor Resolver cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    resolver_base(c, this, args)
}

fn handle(c: &mut Context<'_>, this: RootId) -> Outcome {
    get(c, this, "_handle")
}

pub(crate) fn resolver_cancel(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = handle(c, this)?;
    call_method(c, handle, "cancel", &[])?;
    Ok(c.undefined())
}

pub(crate) fn resolver_get_servers(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = handle(c, this)?;
    let servers = call_method(c, handle, "getServers", &[])?;
    let pairs = if c.is_object_rooted(servers)? { c.array_like_values_rooted(servers)? } else { Vec::new() };
    let mut listed = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let address = get(c, pair, "0")?;
        let address = c.to_string(address)?;
        let port = get(c, pair, "1")?;
        let port = number(c, port).unwrap_or(0.0);
        let text = if port == 0.0 || port == IANA_DNS_PORT {
            address
        } else if ip_family(&address) == IPV6 {
            format!("[{address}]:{port}")
        } else {
            format!("{address}:{port}")
        };
        listed.push(c.string_rooted(&text));
    }
    c.array_rooted(&listed)
}

/// One `setServers` entry as `[family, address, port]`: an address, `[v6]:port` or `v4:port`.
fn server_entry(text: &str) -> Option<(f64, String, f64)> {
    let family = ip_family(text);
    if family != 0.0 {
        return Some((family, text.to_owned(), IANA_DNS_PORT));
    }
    let port_of = |tail: &str| -> Option<f64> {
        if tail.is_empty() {
            return Some(IANA_DNS_PORT);
        }
        let digits = tail.strip_prefix(':')?;
        (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| digits.parse::<f64>().ok().filter(|port| *port != 0.0).unwrap_or(IANA_DNS_PORT))
    };
    if let Some(rest) = text.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            if !host.contains(['[', ']']) && ip_family(host) != 0.0 {
                return Some((ip_family(host), host.to_owned(), port_of(tail).unwrap_or(IANA_DNS_PORT)));
            }
        }
    }
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) => {
            (host, port.parse::<f64>().ok()?)
        }
        _ => (text, IANA_DNS_PORT),
    };
    let family = ip_family(host);
    (family != 0.0).then(|| (family, host.to_owned(), port))
}

pub(crate) fn resolver_set_servers(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let servers = arg(c, args, 0);
    if !c.is_array_rooted(servers)? {
        return Err(super::util_shared_vm::argument_type_error(c, servers, "servers", "Array")?);
    }
    let handle = handle(c, this)?;
    let original = call_method(c, handle, "getServers", &[])?;
    let entries = c.array_like_values_rooted(servers)?;
    let mut parsed = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let text = super::util_shared_vm::validate_string(c, entry, &format!("servers[{index}]"))?;
        let Some((family, address, port)) = server_entry(&text) else {
            return Err(super::node_errors_shared_vm::throw_coded_type_error(c, "ERR_INVALID_IP_ADDRESS", &format!("Invalid IP address: {text}")));
        };
        let family = c.number(family);
        let address = c.string_rooted(&address);
        let port = c.number(port);
        parsed.push(c.array_rooted(&[family, address, port])?);
    }
    let list = c.array_rooted(&parsed)?;
    let status = call_method(c, handle, "setServers", &[list])?;
    if number(c, status).is_some_and(|status| status != 0.0) {
        call_method(c, handle, "setServers", &[original])?;
        let binding = intrinsics(c)?.binding;
        let reason = call_method(c, binding, "strerror", &[status])?;
        let reason = c.to_string(reason)?;
        let servers = c.to_string(servers)?;
        let error = c.error_rooted(&format!("c-ares failed to set servers: \"{reason}\" [{servers}]"))?;
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_DNS_SET_SERVERS_FAILED")?;
        return Err(c.throw(error));
    }
    Ok(c.undefined())
}

pub(crate) fn resolver_set_local_address(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (ipv4, ipv6) = (arg(c, args, 0), arg(c, args, 1));
    super::util_shared_vm::validate_string(c, ipv4, "ipv4")?;
    if !is_undefined(c, ipv6) {
        super::util_shared_vm::validate_string(c, ipv6, "ipv6")?;
    }
    let handle = handle(c, this)?;
    call_method(c, handle, "setLocalAddress", &[ipv4, ipv6])?;
    Ok(c.undefined())
}

/// Settle a promise-delivered request with `outcome`.
fn settle(c: &mut Context<'_>, request: RootId, outcome: Result<RootId, RootId>) -> Result<(), RootedError> {
    let promise = get(c, request, "promise")?;
    match outcome {
        Ok(value) => c.resolve_promise_rooted(promise, value),
        Err(error) => c.reject_promise_rooted(promise, error),
    }
}

/// Deliver a request's outcome: through its callback, or by settling its promise.
fn deliver(c: &mut Context<'_>, request: RootId, outcome: Result<&[RootId], RootId>) -> Result<(), RootedError> {
    let promise = get(c, request, "promise")?;
    if !is_undefined(c, promise) {
        let outcome = match outcome {
            Ok(values) => Ok(values.first().copied().unwrap_or_else(|| c.undefined())),
            Err(error) => Err(error),
        };
        return settle(c, request, outcome);
    }
    let callback = get(c, request, "callback")?;
    let undefined = c.undefined();
    match outcome {
        Ok(values) => {
            let mut arguments = vec![c.null()];
            arguments.extend_from_slice(values);
            c.call_rooted(callback, undefined, &arguments)?;
        }
        Err(error) => {
            c.call_rooted(callback, undefined, &[error])?;
        }
    }
    Ok(())
}

/// A request wrap of `class` whose completion `delivery` decides, and its pending promise.
fn request(c: &mut Context<'_>, class: &str, delivery: Delivery, callback: RootId) -> Result<(RootId, RootId), RootedError> {
    let binding = intrinsics(c)?.binding;
    let class = get(c, binding, class)?;
    let request = c.construct_rooted(class, class, &[])?;
    let result = match delivery {
        Delivery::Promise => {
            let promise = c.pending_promise_rooted()?;
            hide(c, request, "promise", promise)?;
            promise
        }
        Delivery::Callback => {
            put(c, request, "callback", callback)?;
            request
        }
    };
    Ok((request, result))
}

/// A request the binding refused: reported on the next tick (callback) or by rejection.
fn refuse(c: &mut Context<'_>, delivery: Delivery, callback: RootId, promise: RootId, error: RootId) -> Outcome {
    match delivery {
        Delivery::Promise => {
            c.reject_promise_rooted(promise, error)?;
            Ok(promise)
        }
        Delivery::Callback => {
            super::stream_shared_vm::next_tick(c, callback, &[error])?;
            c.object_rooted()
        }
    }
}

pub(crate) fn resolver_query(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let promise_flag = get(c, data, "0")?;
    let delivery = Delivery::of(c.truthy_rooted(promise_flag)?);
    let binding_name = get(c, data, "1")?;
    let binding_text = c.to_string(binding_name)?;
    let name = arg(c, args, 0);
    let mut options = arg(c, args, 1);
    let mut callback = arg(c, args, 2);
    if delivery == Delivery::Callback && c.is_callable_rooted(options)? {
        callback = options;
        options = c.undefined();
    }
    super::util_shared_vm::validate_string(c, name, "name")?;
    if delivery == Delivery::Callback {
        validate_function(c, callback, "callback")?;
    }
    let ttl = c.is_object_rooted(options)? && {
        let ttl = get(c, options, "ttl")?;
        c.truthy_rooted(ttl)?
    };
    let (request, result) = request(c, "QueryReqWrap", delivery, callback)?;
    put(c, request, "bindingName", binding_name)?;
    put(c, request, "hostname", name)?;
    let complete = c.host_function_named(crate::host::shared_vm::operation!("dnsOnResolve"), "onresolve")?;
    put(c, request, "oncomplete", complete)?;
    let ttl = c.boolean(ttl);
    put(c, request, "ttl", ttl)?;
    let handle = handle(c, this)?;
    let status = call_method(c, handle, &binding_text, &[request, name])?;
    if c.truthy_rooted(status)? {
        let error = dns_exception(c, status, &binding_text, Some(name))?;
        if delivery == Delivery::Callback {
            return Err(c.throw(error));
        }
        c.reject_promise_rooted(result, error)?;
    }
    Ok(result)
}

/// `onresolve(err, result, ttls)`, called with the request as receiver.
pub(crate) fn on_resolve(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (status, mut result, ttls) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if c.truthy_rooted(status)? {
        let binding_name = get(c, this, "bindingName")?;
        let binding_name = c.to_string(binding_name)?;
        let hostname = get(c, this, "hostname")?;
        let error = dns_exception(c, status, &binding_name, Some(hostname))?;
        deliver(c, this, Err(error))?;
        return Ok(c.undefined());
    }
    let ttl = get(c, this, "ttl")?;
    if c.truthy_rooted(ttls)? && c.truthy_rooted(ttl)? {
        let addresses = c.array_like_values_rooted(result)?;
        let ttls = c.array_like_values_rooted(ttls)?;
        let mut records = Vec::with_capacity(addresses.len());
        for (index, address) in addresses.into_iter().enumerate() {
            let record = c.object_rooted()?;
            put(c, record, "address", address)?;
            let ttl = ttls.get(index).copied().unwrap_or_else(|| c.undefined());
            put(c, record, "ttl", ttl)?;
            records.push(record);
        }
        result = c.array_rooted(&records)?;
    }
    deliver(c, this, Ok(&[result]))?;
    Ok(c.undefined())
}

pub(crate) fn resolver_resolve(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let promise_flag = c.host_function_data()?;
    let delivery = Delivery::of(c.truthy_rooted(promise_flag)?);
    let hostname = arg(c, args, 0);
    let rrtype = arg(c, args, 1);
    let mut callback = arg(c, args, 2);
    let record = if let Some(text) = c.string_text(rrtype)? {
        match QUERIES.iter().find(|(_, _, record)| !record.is_empty() && *record == text) {
            Some(entry) => entry.0,
            None => return Err(super::util_shared_vm::argument_value_error(c, rrtype, "rrtype", "is invalid")?),
        }
    } else if delivery == Delivery::Callback && c.is_callable_rooted(rrtype)? {
        callback = rrtype;
        "resolve4"
    } else if delivery == Delivery::Promise && is_undefined(c, rrtype) {
        "resolve4"
    } else {
        return Err(super::util_shared_vm::argument_type_error(c, rrtype, "rrtype", "string")?);
    };
    let method = get(c, this, record)?;
    let arguments: Vec<RootId> = match delivery {
        Delivery::Callback => vec![hostname, callback],
        Delivery::Promise => vec![hostname],
    };
    c.call_rooted(method, this, &arguments)
}

/// The settings `lookup(hostname, options)` reads.
struct LookupOptions {
    family: f64,
    hints: f64,
    all: bool,
    order: String,
}

fn one_of_family(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    match number(c, value).filter(|family| FAMILIES.contains(family)) {
        Some(family) => Ok(family),
        None => Err(super::util_shared_vm::argument_value_error(c, value, name, &format!("must be one of: {FAMILY_LIST}"))?),
    }
}

fn lookup_options(c: &mut Context<'_>, options: RootId) -> Result<LookupOptions, RootedError> {
    let order = intrinsic(c, "order")?;
    let mut settings = LookupOptions { family: 0.0, hints: 0.0, all: false, order: c.to_string(order)? };
    if number(c, options).is_some() {
        settings.family = one_of_family(c, options, "family")?;
        return Ok(settings);
    }
    if !c.is_object_rooted(options)? {
        return Ok(settings);
    }
    let hints = get(c, options, "hints")?;
    if !is_nullish(c, hints) {
        let Some(value) = number(c, hints) else {
            return Err(super::util_shared_vm::argument_type_error(c, hints, "options.hints", "number")?);
        };
        settings.hints = f64::from(value as i64 as u32);
        let binding = intrinsics(c)?.binding;
        let mut known = 0u32;
        for name in ["AI_ADDRCONFIG", "AI_ALL", "AI_V4MAPPED"] {
            let flag = get(c, binding, name)?;
            known |= number(c, flag).unwrap_or(0.0) as u32;
        }
        if settings.hints as u32 & !known != 0 {
            let shown = c.number(settings.hints);
            return Err(super::util_shared_vm::argument_value_error(c, shown, "hints", "is invalid")?);
        }
    }
    let family = get(c, options, "family")?;
    if !is_nullish(c, family) {
        settings.family = match c.string_text(family)?.as_deref() {
            Some("IPv4") => IPV4,
            Some("IPv6") => IPV6,
            _ => one_of_family(c, family, "options.family")?,
        };
    }
    let all = get(c, options, "all")?;
    if !is_nullish(c, all) {
        settings.all = validate_boolean(c, all, "options.all")?;
    }
    let verbatim = get(c, options, "verbatim")?;
    if !is_nullish(c, verbatim) {
        let verbatim = validate_boolean(c, verbatim, "options.verbatim")?;
        if verbatim { ORDERS[0].0 } else { ORDERS[1].0 }.clone_into(&mut settings.order);
    }
    let order = get(c, options, "order")?;
    if !is_nullish(c, order) {
        settings.order = validate_order(c, order, "options.order")?;
    }
    Ok(settings)
}

fn validate_order(c: &mut Context<'_>, value: RootId, name: &str) -> Result<String, RootedError> {
    match c.string_text(value)? {
        Some(order) if ORDERS.iter().any(|(known, _)| *known == order) => Ok(order),
        _ => Err(super::util_shared_vm::argument_value_error(c, value, name, &format!("must be one of: {ORDER_LIST}"))?),
    }
}

/// DEP0118, once: a falsy hostname is accepted only for compatibility.
fn warn_invalid_hostname(c: &mut Context<'_>, hostname: RootId) -> Result<(), RootedError> {
    let holder = intrinsics(c)?.holder;
    let warned = get(c, holder, "warned")?;
    if c.truthy_rooted(warned)? {
        return Ok(());
    }
    let yes = c.boolean(true);
    put(c, holder, "warned", yes)?;
    let shown = c.to_string(hostname)?;
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    let message = c.string_rooted(&format!(
        "The provided hostname \"{shown}\" is not a valid hostname, and is supported in the dns module solely for compatibility."
    ));
    let kind = c.string_rooted("DeprecationWarning");
    let code = c.string_rooted("DEP0118");
    call_method(c, process, "emitWarning", &[message, kind, code])?;
    Ok(())
}

fn address_record(c: &mut Context<'_>, address: RootId, family: f64) -> Outcome {
    let record = c.object_rooted()?;
    put(c, record, "address", address)?;
    let family = c.number(family);
    put(c, record, "family", family)?;
    Ok(record)
}

/// An outcome known without a lookup: an empty or numeric hostname.
fn immediate(c: &mut Context<'_>, delivery: Delivery, callback: RootId, values: &[RootId]) -> Outcome {
    match delivery {
        Delivery::Promise => {
            let promise = c.pending_promise_rooted()?;
            let value = values.first().copied().unwrap_or_else(|| c.undefined());
            c.resolve_promise_rooted(promise, value)?;
            Ok(promise)
        }
        Delivery::Callback => {
            let mut arguments = vec![c.null()];
            arguments.extend_from_slice(values);
            super::stream_shared_vm::next_tick(c, callback, &arguments)?;
            c.object_rooted()
        }
    }
}

fn lookup(c: &mut Context<'_>, delivery: Delivery, args: &[RootId]) -> Outcome {
    let hostname = arg(c, args, 0);
    let mut options = arg(c, args, 1);
    let mut callback = arg(c, args, 2);
    if c.truthy_rooted(hostname)? {
        super::util_shared_vm::validate_string(c, hostname, "hostname")?;
    }
    if delivery == Delivery::Callback && c.is_callable_rooted(options)? {
        callback = options;
        options = c.undefined();
    } else if !is_undefined(c, options) && number(c, options).is_none() && !c.is_object_rooted(options)? {
        if delivery == Delivery::Callback {
            let target = if args.len() == 2 { options } else { callback };
            validate_function(c, target, "callback")?;
        }
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, options, "options", "of type object or integer")?);
    } else if delivery == Delivery::Callback {
        validate_function(c, callback, "callback")?;
    }
    let settings = lookup_options(c, options)?;
    if !c.truthy_rooted(hostname)? {
        warn_invalid_hostname(c, hostname)?;
        return if settings.all {
            let empty = c.array_rooted(&[])?;
            immediate(c, delivery, callback, &[empty])
        } else {
            let family = if settings.family == IPV6 { IPV6 } else { IPV4 };
            let null = c.null();
            match delivery {
                Delivery::Promise => {
                    let record = address_record(c, null, family)?;
                    immediate(c, delivery, callback, &[record])
                }
                Delivery::Callback => {
                    let family = c.number(family);
                    immediate(c, delivery, callback, &[null, family])
                }
            }
        };
    }
    let text = c.to_string(hostname)?;
    let matched = ip_family(&text);
    if matched != 0.0 {
        let record = address_record(c, hostname, matched)?;
        return match (delivery, settings.all) {
            (_, true) => {
                let list = c.array_rooted(&[record])?;
                immediate(c, delivery, callback, &[list])
            }
            (Delivery::Promise, false) => immediate(c, delivery, callback, &[record]),
            (Delivery::Callback, false) => {
                let family = c.number(matched);
                immediate(c, delivery, callback, &[hostname, family])
            }
        };
    }
    let (request, result) = request(c, "GetAddrInfoReqWrap", delivery, callback)?;
    let family = c.number(settings.family);
    put(c, request, "family", family)?;
    put(c, request, "hostname", hostname)?;
    let all = c.boolean(settings.all);
    hide(c, request, "all", all)?;
    let complete = c.host_function_named(crate::host::shared_vm::operation!("dnsOnLookup"), if settings.all { "onlookupall" } else { "onlookup" })?;
    put(c, request, "oncomplete", complete)?;
    let binding = intrinsics(c)?.binding;
    let order_constant = ORDERS.iter().find(|(name, _)| *name == settings.order).map_or(ORDERS[0].1, |(_, constant)| constant);
    let order = get(c, binding, order_constant)?;
    let hints = c.number(settings.hints);
    let status = call_method(c, binding, "getaddrinfo", &[request, hostname, family, hints, order])?;
    if c.truthy_rooted(status)? {
        let error = dns_exception(c, status, "getaddrinfo", Some(hostname))?;
        return refuse(c, delivery, callback, result, error);
    }
    Ok(result)
}

/// `onlookup(err, addresses)` / `onlookupall(err, addresses)`, with the request as receiver.
pub(crate) fn on_lookup(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (status, addresses) = (arg(c, args, 0), arg(c, args, 1));
    if c.truthy_rooted(status)? {
        let hostname = get(c, this, "hostname")?;
        let error = dns_exception(c, status, "getaddrinfo", Some(hostname))?;
        deliver(c, this, Err(error))?;
        return Ok(c.undefined());
    }
    let family = get(c, this, "family")?;
    let family = number(c, family).unwrap_or(0.0);
    let family_of = |c: &mut Context<'_>, address: RootId| -> Result<f64, RootedError> {
        if family != 0.0 {
            return Ok(family);
        }
        let text = c.to_string(address)?;
        Ok(ip_family(&text))
    };
    let all = get(c, this, "all")?;
    let promise = get(c, this, "promise")?;
    let promised = !is_undefined(c, promise);
    let entries = c.array_like_values_rooted(addresses)?;
    if c.truthy_rooted(all)? {
        let mut records = Vec::with_capacity(entries.len());
        for address in entries {
            let family = family_of(c, address)?;
            records.push(address_record(c, address, family)?);
        }
        let list = c.array_rooted(&records)?;
        deliver(c, this, Ok(&[list]))?;
        return Ok(c.undefined());
    }
    let address = entries.first().copied().unwrap_or_else(|| c.undefined());
    let resolved_family = family_of(c, address)?;
    if promised {
        let record = address_record(c, address, resolved_family)?;
        deliver(c, this, Ok(&[record]))?;
    } else {
        let family = c.number(resolved_family);
        deliver(c, this, Ok(&[address, family]))?;
    }
    Ok(c.undefined())
}

fn lookup_service(c: &mut Context<'_>, delivery: Delivery, args: &[RootId]) -> Outcome {
    let required = match delivery {
        Delivery::Callback => 3,
        Delivery::Promise => 2,
    };
    if args.len() < required {
        let message = match delivery {
            Delivery::Callback => "The \"address\", \"port\", and \"callback\" arguments must be specified",
            Delivery::Promise => "The \"address\" and \"port\" arguments must be specified",
        };
        return Err(super::node_errors_shared_vm::throw_coded_type_error(c, "ERR_MISSING_ARGS", message));
    }
    let (address, port, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let text = super::util_shared_vm::validate_string(c, address, "address")?;
    if ip_family(&text) == 0.0 {
        return Err(super::util_shared_vm::argument_value_error(c, address, "address", "is invalid")?);
    }
    let port_number = valid_port(c, port)?;
    if delivery == Delivery::Callback {
        validate_function(c, callback, "callback")?;
    }
    let (request, result) = request(c, "GetNameInfoReqWrap", delivery, callback)?;
    put(c, request, "hostname", address)?;
    let port = c.number(port_number);
    put(c, request, "port", port)?;
    let complete = c.host_function_named(crate::host::shared_vm::operation!("dnsOnLookupService"), "onlookupservice")?;
    put(c, request, "oncomplete", complete)?;
    let binding = intrinsics(c)?.binding;
    let status = call_method(c, binding, "getnameinfo", &[request, address, port])?;
    if c.truthy_rooted(status)? {
        let error = dns_exception(c, status, "getnameinfo", Some(address))?;
        return refuse(c, delivery, callback, result, error);
    }
    Ok(result)
}

/// Node's `validatePort(port)` for `lookupService`.
fn valid_port(c: &mut Context<'_>, port: RootId) -> Result<f64, RootedError> {
    let candidate = match number(c, port) {
        Some(value) => Some(value),
        None => match c.string_text(port)? {
            Some(text) if !text.trim().is_empty() => Some(c.to_number(port)?),
            _ => None,
        },
    };
    match candidate {
        Some(value) if value.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(&value) => Ok(value),
        _ => {
            let received = super::util_shared_vm::argument_type_description(c, port)?;
            let limit = u32::from(u16::MAX) + 1;
            let error = c.range_error_rooted(&format!("Port should be >= 0 and < {limit}. Received {received}."))?;
            super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_SOCKET_BAD_PORT")?;
            Err(c.throw(error))
        }
    }
}

/// `onlookupservice(err, hostname, service)`, with the request as receiver.
pub(crate) fn on_lookup_service(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (status, hostname, service) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if c.truthy_rooted(status)? {
        let address = get(c, this, "hostname")?;
        let error = dns_exception(c, status, "getnameinfo", Some(address))?;
        deliver(c, this, Err(error))?;
        return Ok(c.undefined());
    }
    let promise = get(c, this, "promise")?;
    if is_undefined(c, promise) {
        deliver(c, this, Ok(&[hostname, service]))?;
    } else {
        let record = c.object_rooted()?;
        put(c, record, "hostname", hostname)?;
        put(c, record, "service", service)?;
        deliver(c, this, Ok(&[record]))?;
    }
    Ok(c.undefined())
}

pub(crate) fn lookup_callback(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    lookup(c, Delivery::Callback, args)
}

pub(crate) fn lookup_promise(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    lookup(c, Delivery::Promise, args)
}

pub(crate) fn lookup_service_callback(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    lookup_service(c, Delivery::Callback, args)
}

pub(crate) fn lookup_service_promise(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    lookup_service(c, Delivery::Promise, args)
}

pub(crate) fn get_default_result_order(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    intrinsic(c, "order")
}

pub(crate) fn set_default_result_order(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let order = arg(c, args, 0);
    let order = validate_order(c, order, "dnsOrder")?;
    let holder = intrinsics(c)?.holder;
    let order = c.string_rooted(&order);
    put(c, holder, "order", order)?;
    Ok(c.undefined())
}

/// The module `setServers`: the default resolver's servers, which both surfaces share.
pub(crate) fn set_default_servers(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let resolver = intrinsic(c, "defaultResolver")?;
    resolver_set_servers(c, resolver, args)
}

/// The members `dns` and `dns/promises` share, in export order up to the error codes.
fn surface(c: &mut Context<'_>, target: RootId, delivery: Delivery) -> Result<(), RootedError> {
    let (lookup, service, class, resolver) = match delivery {
        Delivery::Callback => ("dnsLookup", "dnsLookupService", "Resolver", "defaultResolver"),
        Delivery::Promise => ("dnsLookupPromise", "dnsLookupServicePromise", "PromisesResolver", "promisesDefaultResolver"),
    };
    let lookup = c.host_function_named(crate::host::shared_vm::operation(lookup), "lookup")?;
    put(c, target, "lookup", lookup)?;
    let service = c.host_function_named(crate::host::shared_vm::operation(service), "lookupService")?;
    put(c, target, "lookupService", service)?;
    let class = intrinsic(c, class)?;
    put(c, target, "Resolver", class)?;
    for (name, operation) in [
        ("getDefaultResultOrder", "dnsGetDefaultResultOrder"),
        ("setDefaultResultOrder", "dnsSetDefaultResultOrder"),
        ("setServers", "dnsSetDefaultServers"),
    ] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, target, name, function)?;
    }
    if delivery == Delivery::Callback {
        let binding = intrinsics(c)?.binding;
        for (name, constant) in [("ADDRCONFIG", "AI_ADDRCONFIG"), ("ALL", "AI_ALL"), ("V4MAPPED", "AI_V4MAPPED")] {
            let value = get(c, binding, constant)?;
            put(c, target, name, value)?;
        }
    }
    for code in ERROR_CODES {
        let value = c.string_rooted(&format!("E{code}"));
        put(c, target, code, value)?;
    }
    let resolver = intrinsic(c, resolver)?;
    for name in DEFAULT_RESOLVER_METHODS {
        let function = get(c, resolver, name)?;
        let bound = call_method(c, function, "bind", &[resolver])?;
        put(c, target, name, bound)?;
    }
    Ok(())
}

pub(crate) fn promises(c: &mut Context<'_>) -> Outcome {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0dns:promises", |c| {
        let module = c.object_rooted()?;
        surface(c, module, Delivery::Promise)?;
        Ok(module)
    })
}

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let module = c.object_rooted()?;
    surface(c, module, Delivery::Callback)?;
    let promises = promises(c)?;
    put(c, module, "promises", promises)?;
    Ok(module)
}
