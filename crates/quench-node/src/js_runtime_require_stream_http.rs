fn require_stream_http_modules(name: &str) -> Option<Value> {
        if name == "readline/promises" || name == "node:readline/promises" {
            let source = r#"(function() {
                function inputLines(input) {
                    if (Array.isArray(input)) return input.map(String);
                    if (typeof input === "string") return input.split("\n").map(function(line) {
                        return line.endsWith("\r") ? line.slice(0, -1) : line;
                    });
                    return [];
                }
                function Interface(options) {
                    options = options || {};
                    this.input = options.input;
                    this.output = options.output;
                    this._closed = false;
                    this._readlineLines = inputLines(this.input);
                    this._readlineIndex = 0;
                }
                Interface.prototype.question = function(prompt) {
                    if (this._closed) return Promise.reject(new Error("Interface is closed"));
                    if (this.output && this.output.write) this.output.write(prompt);
                    var self = this;
                    return new Promise(function(resolve) {
                        if (self.input && self.input.once) self.input.once("line", resolve);
                        else resolve(self._readlineLines[self._readlineIndex++] || "");
                    });
                };
                Interface.prototype[Symbol.asyncIterator] = function() {
                    var self = this;
                    return {
                        next: function() {
                            if (!self._closed && self._readlineIndex < self._readlineLines.length) {
                                return Promise.resolve({
                                    value: self._readlineLines[self._readlineIndex++],
                                    done: false
                                });
                            }
                            return Promise.resolve({ value: undefined, done: true });
                        },
                        return: function() {
                            self.close();
                            return Promise.resolve({ value: undefined, done: true });
                        }
                    };
                };
                Interface.prototype.close = function() {
                    this._closed = true;
                    if (this.input && this.input.pause) this.input.pause();
                };
                Interface.prototype.prompt = function() {};
                Interface.prototype.write = function() {};
                Interface.prototype.pause = function() { return this; };
                Interface.prototype.resume = function() { return this; };
                return { Interface: Interface, createInterface: function(options) {
                    return new Interface(options);
                }};
            })()"#;
            let program = quench_runtime::reduce::reduce_global_script_source(source)
                .map_err(|errors| VmError::EvalError(errors.join("; ")))
                .ok()?;
            let context = quench_runtime::vm::current_context();
            let mut registers = Vec::new();
            let value = quench_runtime::vm::with_current_context(&context, || {
                quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
            }).ok()?;
            return Some(value);
        }
        if name == "diagnostics_channel" || name == "node:diagnostics_channel" {
            let wrapped = format!(
                "(function(module){{{};return module.exports;}})",
                include_str!("modules/diagnostics_channel.js")
            );
            let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
                .map_err(|errors| VmError::EvalError(errors.join("; ")))
                .ok()?;
            let context = quench_runtime::vm::current_context();
            let mut registers = Vec::new();
            let factory = quench_runtime::vm::with_current_context(&context, || {
                quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
            }).ok()?;
            let module = quench_runtime::host_api::object(vec![(
                "exports".into(),
                quench_runtime::host_api::object(vec![]),
            )]);
            quench_runtime::vm::call_value(&factory, &Value::Undefined, &[module.clone()]).ok()?;
            return quench_runtime::execute::get_property_result(&module, "exports").ok();
        }
        if name == "node:test" {
            let test = capability_function(HostCapabilityKind::Custom(CapabilityName::NodeTest));
            return Some(quench_runtime::host_api::object(vec![
                ("test".into(), test.clone()),
                ("before".into(), test.clone()),
                ("after".into(), test),
            ]));
        }
        if name == "node:child_process" || name == "child_process" {
            return Some(Value::object(vec![
                (
                    "execFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildExecFile)),
                ),
                (
                    "spawn".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildSpawn)),
                ),
                (
                    "spawnSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildSpawnSync)),
                ),
                (
                    "fork".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildFork)),
                ),
            ]));
        }
        if name == "stream/promises" || name == "node:stream/promises" {
            return Some(stream_promises_module());
        }
        if name == "stream/consumers" || name == "node:stream/consumers" {
            return Some(quench_runtime::host_api::object(vec![
                (
                    "buffer".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerBuffer,
                    )),
                ),
                (
                    "bytes".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerBytes,
                    )),
                ),
                (
                    "text".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerText,
                    )),
                ),
                (
                    "json".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerJson,
                    )),
                ),
            ]));
        }
        if name == "node:stream" || name == "stream" {
            let stream = capability_function(HostCapabilityKind::Custom(CapabilityName::Stream));
            let stream = quench_runtime::execute::set_property(
                stream,
                "prototype",
                Value::object(vec![(
                    "write".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamBaseWrite,
                    )),
                )]),
            );
            let stream = quench_runtime::execute::set_property(
                stream,
                "call",
                Value::Builtin(quench_runtime::ops::Builtin::Object),
            );
            let readable = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::StreamReadable)),
                "from",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::StreamReadableFrom,
                )),
            );
            let readable = quench_runtime::execute::set_property(
                readable,
                "prototype",
                Value::object(vec![("readableEnded".into(), Value::Boolean(false))]),
            );
            let promises = stream_promises_module();
            let writable = Value::Builtin(quench_runtime::ops::Builtin::Object);
            return Some(Value::object(vec![
                ("Stream".into(), stream),
                (
                    "Transform".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Stream)),
                ),
                ("Readable".into(), readable),
                ("Writable".into(), writable),
                (
                    "Duplex".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamDuplex)),
                ),
                (
                    "PassThrough".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Stream)),
                ),
                (
                    "finished".into(),
                    quench_runtime::execute::get_property_result(&promises, "finished")
                        .unwrap_or(Value::Undefined),
                ),
                ("promises".into(), promises),
                (
                    "pipeline".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamPipeline)),
                ),
                (
                    "addAbortSignal".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamAddAbortSignal,
                    )),
                ),
            ]));
        }
        if name == "node:http" || name == "http" {
            return Some(Value::object(vec![
                (
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpGet)),
                ),
                (
                    "createServer".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpServer)),
                ),
            ]));
        }
        if name == "node:https" || name == "https" {
            let source = r#"(function() {
                const unsupported = (operation) => {
                    throw Object.assign(
                        new Error(operation + " is not supported by quench-node"),
                        { code: "ERR_TLS_NOT_SUPPORTED" }
                    );
                };
                function Agent(options) {
                    this.options = options || {};
                    this.keepAlive = Boolean(this.options.keepAlive);
                    this.scheduling = this.options.scheduling || "lifo";
                    this.rejectUnauthorized = this.options.rejectUnauthorized !== false;
                    this.maxCachedSessions = this.options.maxCachedSessions === undefined
                        ? 100
                        : this.options.maxCachedSessions;
                    this.maxFreeSockets = this.options.maxFreeSockets === undefined
                        ? 256
                        : this.options.maxFreeSockets;
                    this.maxSockets = this.options.maxSockets === undefined
                        ? Infinity
                        : this.options.maxSockets;
                    this.timeout = this.options.timeout || 0;
                    this.defaultPort = 443;
                    this.protocol = "https:";
                    this.freeSockets = {};
                    this.sockets = {};
                    this.requests = {};
                    this.destroy = function() {};
                }
                return {
                    request: function() { return unsupported("https.request"); },
                    get: function() { return unsupported("https.get"); },
                    Agent: Agent,
                    globalAgent: new Agent({ keepAlive: true })
                };
            })()"#;
            let program = quench_runtime::reduce::reduce_global_script_source(source)
                .map_err(|errors| VmError::EvalError(errors.join("; ")))
                .ok()?;
            let context = quench_runtime::vm::current_context();
            let mut registers = Vec::new();
            let value = quench_runtime::vm::with_current_context(&context, || {
                quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
            }).ok()?;
            return Some(value);
        }
    None
}
