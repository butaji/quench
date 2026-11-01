//! `https` (lib/https.js): HTTP over TLS. Its `Server` is a `tls.Server` whose secure
//! connections the HTTP connection listener serves; its `Agent` is an HTTP agent whose
//! connections are `tls.connect` sockets, with a TLS session cache per origin.

use crate::host::NodeHost;
use crate::modules::host_members::{captured, constructor, current, function, members, methods};
use crate::modules::http_shared_vm::server;
use crate::modules::stream_shared_vm::{call_method, get, is_nullish, is_undefined, on, once, put, put_number};
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Https, "httpsMembers", [
    Server,
    CreateServer,
    TlsClientError,
    Close,
    Agent,
    CreateConnection,
    GetName,
    GetSession,
    CacheSession,
    EvictSession,
    Request,
    Get,
    /// The session cache's listeners on an agent's socket.
    SessionArrived,
    SessionClosed,
]);

/// The port and scheme an HTTPS agent defaults to.
const DEFAULT_PORT: f64 = 443.0;
const PROTOCOL: &str = "https:";
/// The sessions an agent caches by default.
const DEFAULT_MAX_CACHED_SESSIONS: f64 = 100.0;
/// The protocol an HTTPS server negotiates by default.
const HTTP1_ALPN: &str = "http/1.1";
/// `http.globalAgent`'s options, which `https.globalAgent` shares.
const GLOBAL_TIMEOUT: f64 = 5000.0;

/// How a TLS option enters an agent's origin name (`Agent.prototype.getName`).
enum NamePart {
    /// The value when truthy.
    Truthy,
    /// The value when defined.
    Defined,
    /// The value as JSON when truthy.
    Json,
    /// The server name, when it differs from the host.
    Servername,
}

const NAME_PARTS: [(&str, NamePart); 20] = [
    ("ca", NamePart::Truthy),
    ("cert", NamePart::Truthy),
    ("clientCertEngine", NamePart::Truthy),
    ("ciphers", NamePart::Truthy),
    ("key", NamePart::Truthy),
    ("pfx", NamePart::Truthy),
    ("rejectUnauthorized", NamePart::Defined),
    ("servername", NamePart::Servername),
    ("minVersion", NamePart::Truthy),
    ("maxVersion", NamePart::Truthy),
    ("secureProtocol", NamePart::Truthy),
    ("crl", NamePart::Truthy),
    ("honorCipherOrder", NamePart::Defined),
    ("ecdhCurve", NamePart::Truthy),
    ("dhparam", NamePart::Truthy),
    ("secureOptions", NamePart::Defined),
    ("sessionIdContext", NamePart::Truthy),
    ("sigalgs", NamePart::Json),
    ("privateKeyIdentifier", NamePart::Truthy),
    ("privateKeyEngine", NamePart::Truthy),
];

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let http = crate::modules::http_shared_vm::module(c)?;
    let tls = crate::modules::tls_shared_vm::module(c)?;
    let tls_server = get(c, tls, "Server")?;
    let server = constructor(c, Https::Server, "Server")?;
    crate::modules::http_shared_vm::inherit(c, server, tls_server)?;
    let prototype = get(c, server, "prototype")?;
    server::install_server_methods(c, prototype)?;
    methods(c, prototype, &[("close", Https::Close)])?;
    let http_agent = get(c, http, "Agent")?;
    let agent = constructor(c, Https::Agent, "Agent")?;
    crate::modules::http_shared_vm::inherit(c, agent, http_agent)?;
    let prototype = get(c, agent, "prototype")?;
    methods(c, prototype, &[
        ("createConnection", Https::CreateConnection),
        ("getName", Https::GetName),
        ("_getSession", Https::GetSession),
        ("_cacheSession", Https::CacheSession),
        ("_evictSession", Https::EvictSession),
    ])?;
    let module = c.object_rooted()?;
    put(c, module, "Agent", agent)?;
    let options = c.object_rooted()?;
    let yes = c.boolean(true);
    put(c, options, "keepAlive", yes)?;
    let lifo = c.string_rooted("lifo");
    put(c, options, "scheduling", lifo)?;
    put_number(c, options, "timeout", GLOBAL_TIMEOUT)?;
    let global_agent = c.construct_rooted(agent, agent, &[options])?;
    put(c, module, "globalAgent", global_agent)?;
    put(c, module, "Server", server)?;
    for (name, member) in [("createServer", Https::CreateServer), ("get", Https::Get), ("request", Https::Request)] {
        let value = function(c, member, name, &[module])?;
        put(c, module, name, value)?;
    }
    Ok(module)
}

fn tls_module(c: &mut Context<'_>) -> Outcome {
    crate::modules::tls_shared_vm::module(c)
}

fn https_class(c: &mut Context<'_>, name: &str) -> Outcome {
    let module = crate::host::shared_vm::commonjs::https_module(c)?;
    get(c, module, name)
}

/// The class a call without `new` constructs, unless it initializes an instance.
fn construct_without_new(c: &mut Context<'_>, this: RootId, name: &str) -> Result<Option<RootId>, RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(None);
    }
    let class = https_class(c, name)?;
    if c.is_object_rooted(this)? && c.instance_of_rooted(this, class)? {
        return Ok(None);
    }
    Ok(Some(class))
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Https>(c)? {
        Https::Server => construct_server(c, this, at(0), at(1)),
        Https::CreateServer => {
            let class = https_class(c, "Server")?;
            c.construct_rooted(class, class, &[at(0), at(1)])
        }
        Https::TlsClientError => {
            let (error, connection) = (at(0), at(1));
            let event = c.string_rooted("clientError");
            let handled = call_method(c, this, "emit", &[event, error, connection])?;
            if !c.truthy_rooted(handled)? {
                call_method(c, connection, "destroy", &[error])?;
            }
            Ok(undefined)
        }
        Https::Close => {
            server::pre_close(c, this)?;
            let tls = tls_module(c)?;
            let tls_server = get(c, tls, "Server")?;
            let prototype = get(c, tls_server, "prototype")?;
            let close = get(c, prototype, "close")?;
            c.call_rooted(close, this, args)?;
            Ok(this)
        }
        Https::Agent => construct_agent(c, this, at(0)),
        Https::CreateConnection => create_connection(c, this, at(0), at(1), at(2)),
        Https::GetName => {
            let name = name_of(c, this, at(0))?;
            Ok(c.string_rooted(&name))
        }
        Https::GetSession => {
            let cache = get(c, this, "_sessionCache")?;
            let map = get(c, cache, "map")?;
            c.get_property_rooted(map, at(0))
        }
        Https::CacheSession => {
            cache_session(c, this, at(0), at(1))?;
            Ok(undefined)
        }
        Https::EvictSession => {
            evict_session(c, this, at(0))?;
            Ok(undefined)
        }
        Https::Request => {
            let [module] = captured(c)?;
            request(c, module, args)
        }
        Https::Get => {
            let [module] = captured(c)?;
            let request = request(c, module, args)?;
            call_method(c, request, "end", &[])?;
            Ok(request)
        }
        Https::SessionArrived => {
            let [agent, key] = captured(c)?;
            cache_session(c, agent, key, at(0))?;
            Ok(undefined)
        }
        Https::SessionClosed => {
            let [agent, key] = captured(c)?;
            if c.truthy_rooted(at(0))? {
                evict_session(c, agent, key)?;
            }
            Ok(undefined)
        }
    }
}

fn construct_server(c: &mut Context<'_>, this: RootId, options: RootId, listener: RootId) -> Outcome {
    if let Some(class) = construct_without_new(c, this, "Server")? {
        return c.construct_rooted(class, class, &[options, listener]);
    }
    let (options, listener) = server::server_options(c, options, listener)?;
    server::store_options(c, this, options)?;
    let tls_options = c.object_rooted()?;
    let yes = c.boolean(true);
    put(c, tls_options, "noDelay", yes)?;
    let protocol = c.string_rooted(HTTP1_ALPN);
    let protocols = c.array_rooted(&[protocol])?;
    put(c, tls_options, "ALPNProtocols", protocols)?;
    for key in c.enumerable_own_keys_rooted(options, true, true)? {
        let value = c.get_property_rooted(options, key)?;
        c.set_property_rooted(tls_options, key, value, tls_options)?;
    }
    let tls = tls_module(c)?;
    let tls_server = get(c, tls, "Server")?;
    let connection = crate::modules::http_shared_vm::connection_listener_function(c)?;
    c.call_rooted(tls_server, this, &[tls_options, connection])?;
    server::initialize_server(c, this, listener)?;
    let client_error = function(c, Https::TlsClientError, "addListener", &[])?;
    on(c, this, "tlsClientError", client_error)?;
    Ok(this)
}

fn construct_agent(c: &mut Context<'_>, this: RootId, options: RootId) -> Outcome {
    if let Some(class) = construct_without_new(c, this, "Agent")? {
        return c.construct_rooted(class, class, &[options]);
    }
    let http = crate::modules::http_shared_vm::module(c)?;
    let http_agent = get(c, http, "Agent")?;
    c.call_rooted(http_agent, this, &[options])?;
    put_number(c, this, "defaultPort", DEFAULT_PORT)?;
    let protocol = c.string_rooted(PROTOCOL);
    put(c, this, "protocol", protocol)?;
    let own = get(c, this, "options")?;
    let cached = get(c, own, "maxCachedSessions")?;
    let cached = if is_undefined(c, cached) { c.number(DEFAULT_MAX_CACHED_SESSIONS) } else { cached };
    put(c, this, "maxCachedSessions", cached)?;
    let cache = c.object_rooted()?;
    let map = c.object_rooted()?;
    put(c, cache, "map", map)?;
    let list = c.array_rooted(&[])?;
    put(c, cache, "list", list)?;
    put(c, this, "_sessionCache", cache)?;
    Ok(this)
}

fn copy_into(c: &mut Context<'_>, target: RootId, source: RootId) -> Result<(), RootedError> {
    for key in c.enumerable_own_keys_rooted(source, true, true)? {
        let value = c.get_property_rooted(source, key)?;
        c.set_property_rooted(target, key, value, target)?;
    }
    Ok(())
}

/// `createConnection(port, host, options)`: a `tls.connect` socket resuming, then caching,
/// its origin's session.
fn create_connection(c: &mut Context<'_>, this: RootId, port: RootId, host: RootId, options: RootId) -> Outcome {
    let mut options = if c.is_object_rooted(port)? {
        port
    } else if c.is_object_rooted(host)? {
        let copy = c.object_rooted()?;
        copy_into(c, copy, host)?;
        copy
    } else {
        let copy = c.object_rooted()?;
        if c.is_object_rooted(options)? {
            copy_into(c, copy, options)?;
        }
        copy
    };
    if c.rooted_value(port).and_then(|value| value.as_number()).is_some() {
        put(c, options, "port", port)?;
    }
    if c.string_text(host)?.is_some() {
        put(c, options, "host", host)?;
    }
    let key = get(c, options, "_agentKey")?;
    let keyed = c.truthy_rooted(key)?;
    if keyed {
        let session = call_method(c, this, "_getSession", &[key])?;
        if c.truthy_rooted(session)? {
            let resumed = c.object_rooted()?;
            put(c, resumed, "session", session)?;
            copy_into(c, resumed, options)?;
            options = resumed;
        }
    }
    let tls = tls_module(c)?;
    let socket = call_method(c, tls, "connect", &[options])?;
    if keyed {
        let arrived = function(c, Https::SessionArrived, "", &[this, key])?;
        on(c, socket, "session", arrived)?;
        let closed = function(c, Https::SessionClosed, "", &[this, key])?;
        once(c, socket, "close", closed)?;
    }
    Ok(socket)
}

fn name_of(c: &mut Context<'_>, this: RootId, options: RootId) -> Result<String, RootedError> {
    let options = if is_nullish(c, options) { c.object_rooted()? } else { options };
    let http = crate::modules::http_shared_vm::module(c)?;
    let http_agent = get(c, http, "Agent")?;
    let prototype = get(c, http_agent, "prototype")?;
    let base = get(c, prototype, "getName")?;
    let name = c.call_rooted(base, this, &[options])?;
    let mut name = c.to_string(name)?;
    for (field, part) in NAME_PARTS {
        name.push(':');
        let value = get(c, options, field)?;
        let shown = match part {
            NamePart::Truthy => c.truthy_rooted(value)?,
            NamePart::Defined => !is_undefined(c, value),
            NamePart::Json => c.truthy_rooted(value)?,
            NamePart::Servername => {
                let host = get(c, options, "host")?;
                c.truthy_rooted(value)? && !c.strict_equal_rooted(value, host)?
            }
        };
        if shown {
            let value = if matches!(part, NamePart::Json) { c.stringify_json_rooted(value)? } else { value };
            name.push_str(&c.to_string(value)?);
        }
    }
    Ok(name)
}

fn cache_session(c: &mut Context<'_>, this: RootId, key: RootId, session: RootId) -> Result<(), RootedError> {
    let maximum = get(c, this, "maxCachedSessions")?;
    let maximum = c.to_number(maximum)?;
    if maximum == 0.0 {
        return Ok(());
    }
    let cache = get(c, this, "_sessionCache")?;
    let map = get(c, cache, "map")?;
    let existing = c.get_property_rooted(map, key)?;
    if c.truthy_rooted(existing)? {
        c.set_property_rooted(map, key, session, map)?;
        return Ok(());
    }
    let list = get(c, cache, "list")?;
    let length = get(c, list, "length")?;
    if c.to_number(length)? >= maximum {
        let oldest = call_method(c, list, "shift", &[])?;
        c.delete_property_rooted(map, oldest)?;
    }
    call_method(c, list, "push", &[key])?;
    c.set_property_rooted(map, key, session, map).map(drop)
}

fn evict_session(c: &mut Context<'_>, this: RootId, key: RootId) -> Result<(), RootedError> {
    let cache = get(c, this, "_sessionCache")?;
    let list = get(c, cache, "list")?;
    let index = call_method(c, list, "indexOf", &[key])?;
    let index = c.to_number(index)?;
    if index < 0.0 {
        return Ok(());
    }
    let start = c.number(index);
    let one = c.number(1.0);
    call_method(c, list, "splice", &[start, one])?;
    let map = get(c, cache, "map")?;
    c.delete_property_rooted(map, key).map(drop)
}

/// `https.request(...args)`: the options of a URL and the given options, with the module's
/// `globalAgent` as the default agent, for a `ClientRequest`.
fn request(c: &mut Context<'_>, module: RootId, args: &[RootId]) -> Outcome {
    let mut rest = args.iter().copied().peekable();
    let mut options = c.object_rooted()?;
    if let Some(&first) = rest.peek() {
        if c.string_text(first)?.is_some() {
            rest.next();
            let global = c.global_root()?;
            let url_class = get(c, global, "URL")?;
            let url = c.construct_rooted(url_class, url_class, &[first])?;
            let undefined = c.undefined();
            options = crate::modules::url_shared_vm::url_to_http_options(c, undefined, &[url])?;
        } else if crate::modules::url_shared_vm::is_url_like(c, first)? {
            rest.next();
            let undefined = c.undefined();
            options = crate::modules::url_shared_vm::url_to_http_options(c, undefined, &[first])?;
        }
    }
    if let Some(&next) = rest.peek() {
        if c.truthy_rooted(next)? && !c.is_callable_rooted(next)? {
            rest.next();
            copy_into(c, options, next)?;
        }
    }
    let default_agent = get(c, module, "globalAgent")?;
    put(c, options, "_defaultAgent", default_agent)?;
    let mut values = vec![options];
    values.extend(rest);
    let http = crate::modules::http_shared_vm::module(c)?;
    let class = get(c, http, "ClientRequest")?;
    c.construct_rooted(class, class, &values)
}
