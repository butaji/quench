//! `_stream_wrap`: Node's deprecated `JSStreamSocket`, a `net.Socket` over any Duplex stream.
//! The class is a native constructor that extends `net.Socket` the way TLSSocket does; its
//! methods and the stream listeners are host functions over the wrapped stream.
use crate::host::NodeHost;
use crate::modules::host_members::{captured, constructor, current, function, leading, members, methods};
use crate::modules::stream_shared_vm::{get, inherit, is_undefined};
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a>;
type Outcome = Result<RootId, RootedError>;

/// Node's DEP0125 warning message and code.
const DEPRECATION: (&str, &str) = ("The _stream_wrap module is deprecated.", "DEP0125");

members!(Member, "streamWrapSocket", [
    /// `new JSStreamSocket(stream)`.
    Construct,
    /// `_read()`: the wrapped stream resumes.
    Read,
    /// `_write(chunk, encoding, callback)`: the stream writes the chunk.
    Write,
    /// `_final(callback)`: the stream ends, then the socket's final step completes.
    Final,
    /// `_destroy(error, callback)`: the stream is destroyed, then `net.Socket`'s `_destroy`.
    Destroy,
    /// The stream's 'data' listener: the chunk becomes readable on the socket.
    Data,
    /// The stream's 'end' listener: the socket's readable side ends.
    End,
    /// The stream's 'error' listener: the socket is destroyed with the error.
    Error,
    /// The stream's 'close' listener: the socket is destroyed.
    Close,
]);

/// The `_stream_wrap` builtin: its export is the `JSStreamSocket` class itself. The class is
/// built once and cached, so the deprecation warning is emitted once.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0_stream_wrap", |c| {
        warn_deprecated(c)?;
        let net = crate::host::shared_vm::commonjs::net_module(c)?;
        let base = get(c, net, "Socket")?;
        let class = constructor(c, Member::Construct, "JSStreamSocket")?;
        let prototype = get(c, class, "prototype")?;
        methods(c, prototype, &[
            ("_read", Member::Read),
            ("_write", Member::Write),
            ("_final", Member::Final),
            ("_destroy", Member::Destroy),
        ])?;
        inherit(c, class, base)?;
        Ok(class)
    })
}

/// The arguments of `_write(chunk, encoding, callback)`, the callback last.
const WRITE_ARGUMENTS: usize = 3;
/// The arguments of `_destroy(error, callback)`.
const DESTROY_ARGUMENTS: usize = 2;
/// The captures of a stream listener: the socket, then the stream.
const LISTENER_CAPTURES: usize = 2;

/// Dispatches a call of any `Member` function: the constructor, a prototype method, or a
/// stream listener (whose captures are the socket and the stream).
pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    match current::<Member>(c)? {
        Member::Construct => construct(c, args),
        listener @ (Member::Data | Member::End | Member::Error | Member::Close) => listener_call(c, listener, args),
        method => prototype_method(c, method, this, args),
    }
}

/// `_read`, `_write`, `_final` and `_destroy` on the socket, `this`, over the wrapped stream.
fn prototype_method(c: &mut Context<'_>, method: Member, this: RootId, args: &[RootId]) -> Outcome {
    match method {
        Member::Read => {
            let stream = get(c, this, "stream")?;
            call(c, stream, "resume", &[])
        }
        Member::Write => {
            let ([chunk, encoding, callback], _) = leading::<WRITE_ARGUMENTS>(c, args);
            let stream = get(c, this, "stream")?;
            call(c, stream, "write", &[chunk, encoding, callback])
        }
        Member::Final => {
            let ([callback], _) = leading::<1>(c, args);
            let stream = get(c, this, "stream")?;
            call(c, stream, "end", &[])?;
            settle(c, callback, &[])
        }
        Member::Destroy => {
            let ([error, callback], _) = leading::<DESTROY_ARGUMENTS>(c, args);
            let stream = get(c, this, "stream")?;
            call(c, stream, "destroy", &[])?;
            let base = socket_base(c)?;
            let prototype = get(c, base, "prototype")?;
            let parent = get(c, prototype, "_destroy")?;
            c.call_rooted(parent, this, &[error, callback])
        }
        _ => unreachable!("not a prototype method"),
    }
}

/// The stream's 'data', 'end', 'error' and 'close' listeners, over the socket and the stream.
fn listener_call(c: &mut Context<'_>, listener: Member, args: &[RootId]) -> Outcome {
    match listener {
        Member::Data => {
            let [socket, stream] = captured::<LISTENER_CAPTURES>(c)?;
            let ([chunk], _) = leading::<1>(c, args);
            let pushed = call(c, socket, "push", &[chunk])?;
            if !c.truthy_rooted(pushed)? {
                call(c, stream, "pause", &[])?;
            }
            Ok(c.undefined())
        }
        Member::End => {
            let [socket, _] = captured::<LISTENER_CAPTURES>(c)?;
            let null = c.null();
            call(c, socket, "push", &[null])?;
            Ok(c.undefined())
        }
        Member::Error => {
            let [socket, _] = captured::<LISTENER_CAPTURES>(c)?;
            let ([error], _) = leading::<1>(c, args);
            call(c, socket, "destroy", &[error])?;
            Ok(c.undefined())
        }
        Member::Close => {
            let [socket, _] = captured::<LISTENER_CAPTURES>(c)?;
            call(c, socket, "destroy", &[])
        }
        _ => unreachable!("not a stream listener"),
    }
}

/// `new JSStreamSocket(stream)`: a `net.Socket` whose prototype is the new target's, with
/// the stream's events wired to it.
fn construct(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let target = c.new_target_rooted().unwrap_or(undefined);
    if is_undefined(c, target) {
        let error = c.type_error_rooted("Class constructor JSStreamSocket cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let ([stream], _) = leading::<1>(c, args);
    let base = socket_base(c)?;
    let options = c.object_rooted()?;
    let socket = c.construct_rooted(base, base, &[options])?;
    let prototype = get(c, target, "prototype")?;
    c.set_prototype_rooted(socket, prototype)?;
    c.assign_named_property_rooted(socket, "stream", stream)?;
    for (event, listener) in [
        ("data", Member::Data),
        ("end", Member::End),
        ("error", Member::Error),
        ("close", Member::Close),
    ] {
        let handler = function(c, listener, "", &[socket, stream])?;
        let key = c.string_rooted(event);
        call(c, stream, "on", &[key, handler])?;
    }
    Ok(socket)
}

/// The public `net.Socket` constructor, the base every JSStreamSocket extends.
fn socket_base(c: &mut Context<'_>) -> Outcome {
    let net = crate::host::shared_vm::commonjs::net_module(c)?;
    get(c, net, "Socket")
}

/// `object[name](...args)`, with `object` as the receiver.
fn call(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Outcome {
    let callee = get(c, object, name)?;
    c.call_rooted(callee, object, args)
}

/// Runs a Node callback with `undefined` as its receiver.
fn settle(c: &mut Context<'_>, callback: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, args)
}

fn warn_deprecated(c: &mut Context<'_>) -> Result<(), RootedError> {
    let process = c
        .host_as::<crate::host::NodeHost>()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    let emit = c.get_named_property_rooted(process, "emitWarning")?;
    let message = c.string_rooted(DEPRECATION.0);
    let kind = c.string_rooted("DeprecationWarning");
    let code = c.string_rooted(DEPRECATION.1);
    c.call_rooted(emit, process, &[message, kind, code])?;
    Ok(())
}
