//! Lazily created stdin and nonblocking host-loop admission of OS input.
#[cfg(unix)]
mod platform {
    use crate::host::NodeHost;
    use quench_runtime::{NativeContext, RootId, RootedError, Runtime, Value};

    const STDIN_FD: libc::c_int = 0;
    const READ_CHUNK_BYTES: usize = 64 * 1024;

    #[derive(Default)]
    pub(crate) struct State {
        stream: Option<RootId>,
        pump: Option<RootId>,
        requested: bool,
        ended: bool,
        referenced: bool,
        pending: Option<std::io::Result<Vec<u8>>>,
    }

    pub(crate) fn install(
        context: &mut NativeContext<'_, NodeHost>,
        process: RootId,
    ) -> Result<(), RootedError> {
        let open = context.host_function_with_data(
            crate::host::shared_vm::operation("processOpenStdin"),
            process,
        )?;
        let factory = context.evaluate_script_rooted(
            "operation => function() { return operation(); }",
            "node:process/open-stdin.js",
        )?;
        let undefined = context.undefined();
        let open = context.call_rooted(factory, undefined, &[open])?;
        set(context, process, "openStdin", open)?;
        let getter = context.host_function(crate::host::shared_vm::operation("processStdin"))?;
        let global = context.global_root()?;
        let key = context.string_rooted("Object");
        let object = context.get_property_rooted(global, key)?;
        let key = context.string_rooted("defineProperty");
        let define = context.get_property_rooted(object, key)?;
        let descriptor = context.object_rooted()?;
        set(context, descriptor, "get", getter)?;
        let yes = context.boolean(true);
        set(context, descriptor, "configurable", yes)?;
        set(context, descriptor, "enumerable", yes)?;
        let key = context.string_rooted("stdin");
        context.call_rooted(define, object, &[process, key, descriptor])?;
        Ok(())
    }

    pub(crate) fn open(
        context: &mut NativeContext<'_, NodeHost>,
        _: RootId,
        _: &[RootId],
    ) -> Result<RootId, RootedError> {
        let process = context.host_function_data()?;
        let key = context.string_rooted("stdin");
        let stream = context.get_property_rooted(process, key)?;
        let key = context.string_rooted("resume");
        let resume = context.get_property_rooted(stream, key)?;
        context.call_rooted(resume, stream, &[])?;
        let key = context.string_rooted("stdin");
        context.get_property_rooted(process, key)
    }

    pub(crate) fn get(
        context: &mut NativeContext<'_, NodeHost>,
        _: RootId,
        _: &[RootId],
    ) -> Result<RootId, RootedError> {
        let state = context.host_mut().shared_state();
        if let Some(stream) = state.borrow().stdin.stream {
            return Ok(stream);
        }
        let module = crate::host::shared_vm::commonjs::stream_module(context)?;
        let key = context.string_rooted("Readable");
        let readable = context.get_property_rooted(module, key)?;
        let options = context.object_rooted()?;
        let read = context.host_function(crate::host::shared_vm::operation("processStdinRead"))?;
        set(context, options, "read", read)?;
        let destroy =
            context.host_function(crate::host::shared_vm::operation("processStdinDestroy"))?;
        set(context, options, "destroy", destroy)?;
        let stream = context.construct_rooted(readable, readable, &[options])?;
        let fd = context.number(f64::from(STDIN_FD));
        set(context, stream, "fd", fd)?;
        let key = context.string_rooted("pause");
        let pause = context.get_property_rooted(stream, key)?;
        let pause = context.host_function_with_data(
            crate::host::shared_vm::operation("processStdinPause"),
            pause,
        )?;
        set(context, stream, "pause", pause)?;
        for (name, reference) in [("ref", true), ("unref", false)] {
            let data = context.boolean(reference);
            let function = context.host_function_with_data(
                crate::host::shared_vm::operation("processStdinRef"),
                data,
            )?;
            set(context, stream, name, function)?;
        }
        let pump = context.host_function_with_data(
            crate::host::shared_vm::operation("processStdinPump"),
            stream,
        )?;
        let stream = context.retain(stream)?;
        let pump = context.retain(pump)?;
        state.borrow_mut().stdin = State {
            stream: Some(stream),
            pump: Some(pump),
            referenced: true,
            ..State::default()
        };
        Ok(stream)
    }

    pub(crate) fn read(
        context: &mut NativeContext<'_, NodeHost>,
        _: RootId,
        _: &[RootId],
    ) -> Result<RootId, RootedError> {
        context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .stdin
            .requested = true;
        Ok(context.undefined())
    }

    pub(crate) fn pause(
        context: &mut NativeContext<'_, NodeHost>,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .stdin
            .requested = false;
        let operation = context.host_function_data()?;
        context.call_rooted(operation, receiver, args)
    }

    pub(crate) fn reference(
        context: &mut NativeContext<'_, NodeHost>,
        receiver: RootId,
        _: &[RootId],
    ) -> Result<RootId, RootedError> {
        let data = context.host_function_data()?;
        let referenced = context.truthy_rooted(data)?;
        context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .stdin
            .referenced = referenced;
        Ok(receiver)
    }

    pub(crate) fn destroy(
        context: &mut NativeContext<'_, NodeHost>,
        _: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        {
            let shared = context.host_mut().shared_state();
            let mut state = shared.borrow_mut();
            state.stdin.ended = true;
            state.stdin.requested = false;
            state.stdin.pending = None;
        }
        let undefined = context.undefined();
        let error = args.first().copied().unwrap_or(undefined);
        if let Some(callback) = args.get(1).copied() {
            context.call_rooted(callback, undefined, &[error])?;
        }
        Ok(undefined)
    }

    pub(crate) fn has_work(state: &State) -> bool {
        state.requested && !state.ended && state.referenced
    }

    pub(crate) fn poll(runtime: &mut Runtime<NodeHost>) -> Result<bool, String> {
        let state = runtime.host_mut().shared_state();
        let callback = {
            let state = state.borrow();
            if !state.stdin.requested || state.stdin.ended {
                return Ok(false);
            }
            state.stdin.pump
        };
        let Some(callback) = callback else {
            return Ok(false);
        };
        let mut descriptor = libc::pollfd {
            fd: STDIN_FD,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut descriptor, 1, 0) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(error.to_string());
        }
        if ready == 0 {
            return Ok(false);
        }
        // One queued read per readiness notification. Callbacks and errors are
        // routed by the same checkpoint authority as timers and network callbacks.
        let mut bytes = vec![0; READ_CHUNK_BYTES];
        let pending = loop {
            let length = unsafe { libc::read(STDIN_FD, bytes.as_mut_ptr().cast(), bytes.len()) };
            if length >= 0 {
                bytes.truncate(length as usize);
                break Ok(bytes);
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(false);
            }
            break Err(error);
        };
        {
            let mut state = state.borrow_mut();
            state.stdin.requested = false;
            state.stdin.pending = Some(pending);
        }
        let callback = runtime.root(
            runtime
                .rooted_value(callback)
                .ok_or("stdin pump root missing")?,
        );
        let receiver = runtime.root(Value::UNDEFINED);
        state.borrow_mut().scheduler.queue_shared_next_tick(
            crate::modules::shared_event_loop::SharedCallback {
                execution: None,
                callback,
                receiver,
                args: Vec::new(),
            },
        );
        Ok(true)
    }

    pub(crate) fn pump(
        context: &mut NativeContext<'_, NodeHost>,
        _: RootId,
        _: &[RootId],
    ) -> Result<RootId, RootedError> {
        let stream = context.host_function_data()?;
        let key = context.string_rooted("destroyed");
        let destroyed = context.get_property_rooted(stream, key)?;
        if context.truthy_rooted(destroyed)? {
            context.host_mut().shared_state().borrow_mut().stdin.ended = true;
            return Ok(context.undefined());
        }
        let pending = context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .stdin
            .pending
            .take()
            .ok_or_else(|| RootedError::host("stdin completion has no pending input"))?;
        let bytes = match pending {
            Ok(bytes) => bytes,
            Err(error) => {
                context.host_mut().shared_state().borrow_mut().stdin.ended = true;
                let code = error.raw_os_error().map(|errno| {
                    super::super::util_system_error_shared_vm::system_error_name(-f64::from(errno))
                });
                let message = code
                    .as_ref()
                    .map_or_else(|| error.to_string(), |code| format!("read {code}"));
                let exception = context.error_rooted(&message)?;
                if let Some(code) = code {
                    let value = context.string_rooted(&code);
                    set(context, exception, "code", value)?;
                    let value = context.number(-f64::from(error.raw_os_error().unwrap()));
                    set(context, exception, "errno", value)?;
                    let value = context.string_rooted("read");
                    set(context, exception, "syscall", value)?;
                }
                let key = context.string_rooted("destroy");
                let destroy = context.get_property_rooted(stream, key)?;
                context.call_rooted(destroy, stream, &[exception])?;
                return Ok(context.undefined());
            }
        };
        let value = if bytes.is_empty() {
            context.host_mut().shared_state().borrow_mut().stdin.ended = true;
            context.null()
        } else {
            let buffer = crate::host::shared_vm::commonjs::buffer_module(context)?;
            let key = context.string_rooted("Buffer");
            let buffer = context.get_property_rooted(buffer, key)?;
            let key = context.string_rooted("from");
            let from = context.get_property_rooted(buffer, key)?;
            let values = bytes
                .iter()
                .map(|byte| context.number(f64::from(*byte)))
                .collect::<Vec<_>>();
            let array = context.array_rooted(&values)?;
            context.call_rooted(from, buffer, &[array])?
        };
        let key = context.string_rooted("push");
        let push = context.get_property_rooted(stream, key)?;
        context.call_rooted(push, stream, &[value])?;
        Ok(context.undefined())
    }

    fn set(
        context: &mut NativeContext<'_, NodeHost>,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<(), RootedError> {
        let key = context.string_rooted(name);
        if context.set_property_rooted(object, key, value, object)? {
            Ok(())
        } else {
            Err(RootedError::host(format!("cannot install stdin {name}")))
        }
    }
}
#[cfg(unix)]
pub(crate) use platform::*;

#[cfg(not(unix))]
mod platform {
    use crate::host::NodeHost;
    use quench_runtime::{NativeContext, RootId, RootedError, Runtime};
    #[derive(Default)]
    pub(crate) struct State;
    pub(crate) fn install(
        _: &mut NativeContext<'_, NodeHost>,
        _: RootId,
    ) -> Result<(), RootedError> {
        Ok(())
    }
    pub(crate) fn has_work(_: &State) -> bool {
        false
    }
    pub(crate) fn poll(_: &mut Runtime<NodeHost>) -> Result<bool, String> {
        Ok(false)
    }
    pub(crate) fn unsupported(
        _: &mut NativeContext<'_, NodeHost>,
        _: RootId,
        _: &[RootId],
    ) -> Result<RootId, RootedError> {
        Err(RootedError::host(
            "native stdin polling is unavailable on this platform",
        ))
    }
    pub(crate) use unsupported as open;
    pub(crate) use unsupported as get;
    pub(crate) use unsupported as read;
    pub(crate) use unsupported as pause;
    pub(crate) use unsupported as reference;
    pub(crate) use unsupported as pump;
    pub(crate) use unsupported as destroy;
}
#[cfg(not(unix))]
pub(crate) use platform::*;
