//! Native TCP binding over Transport's actual socket ownership.
use super::{event_constructor_shared_vm::get, net_shared_vm as net};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
fn id(c: &mut Context<'_>, o: RootId) -> Result<u64, RootedError> {
    match super::net_reference_shared_vm::handle_id(c, o) {
        Ok(id) => Ok(id),
        Err(_) => {
            let e = c.type_error_rooted("Illegal invocation")?;
            Err(c.throw(e))
        }
    }
}
fn status(c: &mut Context<'_>, r: std::io::Result<()>) -> RootId {
    c.number(
        r.err()
            .map_or(0, |e| -e.raw_os_error().unwrap_or(libc::EINVAL)) as f64,
    )
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:tcp_wrap", |c| {
        let b = c.object_rooted()?;
        let tcp = c.host_constructor_named(
            crate::host::shared_vm::operation("nativeTcpConstruct"),
            "TCP",
        )?;
        let tp = get(c, tcp, "prototype")?;
        let libuv = c.host_constructor_named(
            crate::host::shared_vm::operation("nativeTcpAbstract"),
            "LibuvStreamWrap",
        )?;
        let lp = get(c, libuv, "prototype")?;
        let handle = c.host_constructor_named(
            crate::host::shared_vm::operation("nativeTcpAbstract"),
            "HandleWrap",
        )?;
        let hp = get(c, handle, "prototype")?;
        c.set_prototype_rooted(tp, lp)?;
        c.set_prototype_rooted(lp, hp)?;
        for (p, n, op) in [
            (tp,"connect","nativeStreamConnect"),
            (tp,"connect6","nativeStreamConnect6"),
            (lp,"shutdown","nativeStreamShutdown"),
            (lp, "writeBuffer", "nativeStreamWriteBuffer"),
            (lp, "readStart", "nativeStreamReadStart"),
            (lp, "readStop", "nativeStreamReadStop"),
            (tp, "bind", "nativeTcpBind"),
            (tp, "bind6", "nativeTcpBind6"),
            (tp, "listen", "nativeTcpListen"),
            (tp, "getsockname", "nativeTcpName"),
            (hp, "close", "nativeTcpClose"),
            (hp, "ref", "nativeTcpRef"),
            (hp, "unref", "nativeTcpUnref"),
            (hp, "hasRef", "nativeTcpHasRef"),
        ] {
            let f = c.host_function_named(crate::host::shared_vm::operation(op), n)?;
            put(c, p, n, f)?;
        }
        let getter = c.host_function(crate::host::shared_vm::operation("nativeTcpQueue"))?;
        let d = c.null_object_rooted()?;
        put(c, d, "get", getter)?;
        let yes = c.boolean(true);
        put(c, d, "enumerable", yes)?;
        let k = c.string_rooted("writeQueueSize");
        c.define_property_descriptor_rooted(lp, k, d)?;
        put(c, b, "TCP", tcp)?;
        let connect=c.host_constructor_named(crate::host::shared_vm::operation("nativeStreamConnectWrap"),"TCPConnectWrap")?;
        put(c,b,"TCPConnectWrap",connect)?;
        let constants = c.object_rooted()?;
        for (n, v) in [
            ("SOCKET", 0),
            ("SERVER", 1),
            ("UV_TCP_IPV6ONLY", 1),
            ("UV_TCP_REUSEPORT", 2),
        ] {
            let v = c.number(v as f64);
            put(c, constants, n, v)?;
        }
        put(c, b, "constants", constants)?;
        Ok(b)
    })
}
pub(crate) fn prototype(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let b = binding(c)?;
    let tcp = get(c, b, "TCP")?;
    get(c, tcp, "prototype")
}
pub(crate) fn abstract_constructor(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let e = c.type_error_rooted("Illegal constructor")?;
    Err(c.throw(e))
}
pub(crate) fn construct(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let kind = args
        .first()
        .and_then(|r| c.rooted_value(*r))
        .and_then(|v| v.as_number());
    if !matches!(kind, Some(0.0) | Some(1.0)) {
        let e = c.type_error_rooted("Invalid TCP socket type")?;
        return Err(c.throw(e));
    }
    let shared = c.host_mut().shared_state();
    let endpoint = net::native_tcp_create(&mut shared.borrow_mut().tcp)
        .map_err(|e| RootedError::host(e.to_string()))?;
    super::net_reference_shared_vm::register_identity(c, receiver, endpoint)?;
    let reading = c.boolean(false);
    put(c, receiver, "reading", reading)?;
    let null = c.null();
    put(c, receiver, "onconnection", null)?;
    let owner = super::net_reference_shared_vm::owner_symbol(c)?;
    c.set_property_rooted(receiver, owner, null, receiver)?;
    Ok(receiver)
}
fn bind(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    ipv6: bool,
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let input = args.first().copied().unwrap_or_else(|| c.undefined());
    let text = c.to_string(input)?;
    let ip = match text.parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_ipv6() == ipv6 => ip,
        _ => return Ok(c.number(-f64::from(libc::EINVAL))),
    };
    let port = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let port = c.to_number(port)? as u16;
    let flags = if let Some(r) = args.get(2) {
        c.to_number(*r)? as u32
    } else {
        0
    };
    let shared = c.host_mut().shared_state();
    let result = net::native_tcp_bind(
        &mut shared.borrow_mut().tcp,
        endpoint,
        std::net::SocketAddr::new(ip, port),
        flags,
    );
    Ok(status(c, result))
}
pub(crate) fn bind4(c: &mut Context<'_>, r: RootId, a: &[RootId]) -> Result<RootId, RootedError> {
    bind(c, r, a, false)
}
pub(crate) fn bind6(c: &mut Context<'_>, r: RootId, a: &[RootId]) -> Result<RootId, RootedError> {
    bind(c, r, a, true)
}
pub(crate) fn listen(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let backlog = args.first().copied().unwrap_or_else(|| c.undefined());
    let backlog = net::convert_listen_backlog(c, backlog)?;
    let shared = c.host_mut().shared_state();
    let result = net::native_tcp_listen(&mut shared.borrow_mut().tcp, endpoint, backlog);
    if result.is_ok() && !shared.borrow().native_tcp_listeners.contains_key(&endpoint) {
        let root = c.retain(receiver)?;
        shared
            .borrow_mut()
            .native_tcp_listeners
            .insert(endpoint, root);
    }
    Ok(status(c, result))
}
pub(crate) fn name(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let result = net::native_tcp_address(&shared.borrow().tcp, endpoint);
    match result {
        Ok(a) => {
            let out = args.first().copied().unwrap_or_else(|| c.undefined());
            let v = c.string_rooted(&a.ip().to_string());
            put(c, out, "address", v)?;
            let v = c.string_rooted(if a.is_ipv4() { "IPv4" } else { "IPv6" });
            put(c, out, "family", v)?;
            let v = c.number(a.port() as f64);
            put(c, out, "port", v)?;
            Ok(c.number(0.0))
        }
        Err(e) => Ok(status(c, Err(e))),
    }
}
pub(crate) fn queue(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let n = net::socket_write_queue_size(&shared.borrow().tcp, endpoint);
    Ok(c.number((n as u32) as f64))
}
pub(crate) fn close(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    if !net::native_tcp_is_open(&shared.borrow().tcp, endpoint) {
        return Ok(c.undefined());
    }
    let key = super::net_reference_shared_vm::onclose_symbol(c)?;
    if let Some(callback) = args.first() {
        if c.is_callable_rooted(*callback)? {
            c.set_property_rooted(receiver, key, *callback, receiver)?;
        }
    }
    let root = c.retain(receiver)?;
    shared
        .borrow_mut()
        .net_handle_close_callbacks
        .insert(endpoint, root);
    super::stream_binding::release_reader(c, endpoint);
    net::native_tcp_close(&mut shared.borrow_mut().tcp, endpoint);
    let listening = shared.borrow_mut().native_tcp_listeners.remove(&endpoint);
    if let Some(root) = listening {
        c.release_root(root);
    }
    Ok(c.undefined())
}
fn reference(c: &mut Context<'_>, receiver: RootId, value: bool) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    net::native_tcp_reference(&mut shared.borrow_mut().tcp, endpoint, value);
    Ok(c.undefined())
}
pub(crate) fn reference_on(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    reference(c, r, true)
}
pub(crate) fn reference_off(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    reference(c, r, false)
}
pub(crate) fn has_ref(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let v = net::native_tcp_has_ref(&shared.borrow().tcp, endpoint);
    Ok(c.boolean(v))
}
