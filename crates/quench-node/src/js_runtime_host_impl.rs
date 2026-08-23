fn query_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let raw = input.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'+' {
            bytes.push(b' ');
            index += 1;
        } else if raw[index] == b'%' && index + 2 < raw.len() {
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            if let (Some(high), Some(low)) = (hex(raw[index + 1]), hex(raw[index + 2])) {
                bytes.push(high * 16 + low);
                index += 3;
            } else {
                bytes.push(raw[index]);
                index += 1;
            }
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn query_pairs(input: &str) -> Vec<(String, String)> {
    input
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.find('=') {
            Some(index) => (
                query_decode(&pair[..index]),
                query_decode(&pair[index + 1..]),
            ),
            None => (query_decode(pair), String::new()),
        })
        .collect()
}

impl Host for QuenchNodeHost {
    fn call(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        // Bootstrap and older module paths can retain the legacy HTTP registry
        // IDs. Normalize them at the active dispatch boundary so they reach
        // the same handlers as the dedicated capability IDs.
        let capability = HostCapabilityRef {
            realm: capability.realm,
            kind: match capability.kind {
                HostCapabilityKind::Custom(1) | HostCapabilityKind::Custom(0x1200) => {
                    HostCapabilityKind::Custom(CapabilityName::Require)
                }
                HostCapabilityKind::Custom(1805) => {
                    HostCapabilityKind::Custom(CapabilityName::HttpGet)
                }
                HostCapabilityKind::Custom(0x0F01) => {
                    HostCapabilityKind::Custom(CapabilityName::HttpGet)
                }
                HostCapabilityKind::Custom(0x0F02) => {
                    HostCapabilityKind::Custom(CapabilityName::HttpServer)
                }
                HostCapabilityKind::Custom(0x0F03) => HostCapabilityKind::Custom(501),
                HostCapabilityKind::Custom(0x0F04) => HostCapabilityKind::Custom(502),
                HostCapabilityKind::Custom(0x0F05) => HostCapabilityKind::Custom(503),
                HostCapabilityKind::Custom(0x0F06) => HostCapabilityKind::Custom(504),
                HostCapabilityKind::Custom(0x0F07) => HostCapabilityKind::Custom(507),
                HostCapabilityKind::Custom(0x0F08) => HostCapabilityKind::Custom(508),
                HostCapabilityKind::Custom(0x0F09) => HostCapabilityKind::Custom(509),
                HostCapabilityKind::Custom(0x0F0A) => HostCapabilityKind::Custom(510),
                HostCapabilityKind::Custom(0x0F0B) => HostCapabilityKind::Custom(511),
                HostCapabilityKind::Custom(0x0F0C) => HostCapabilityKind::Custom(512),
                HostCapabilityKind::Custom(0x0F0D) => HostCapabilityKind::Custom(513),
                kind => kind,
            },
        };
        // Bound legacy `__quench_require_for__` capabilities can arrive
        // through the generic host path; dispatch it before family handlers
        // that may claim an unknown capability.
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::RequireFor) {
            return require_module(arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(0x070D) {
            return Ok(Value::Undefined);
        }
        if let Some(result) = self.dispatch_tmpdir(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_misc_e(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_misc_d(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_misc_c(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_misc_b(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_misc_a(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_url(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_crypto_c(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_crypto_b(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_crypto_a(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_buffer(capability, receiver, arguments) {
            return result;
        }
        if let Some(result) = self.dispatch_core(capability, receiver, arguments) {
            return result;
        }
        match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::Require) => require_module(arguments),
            HostCapabilityKind::Custom(CapabilityName::TimerImmediate | CapabilityName::Timer) => {
                NODE_TIMER_COUNTS.with(|counts| {
                    let (timeouts, immediates) = counts.get();
                    if capability.kind == HostCapabilityKind::Custom(CapabilityName::TimerImmediate)
                    {
                        counts.set((timeouts, immediates + 1));
                    } else {
                        counts.set((timeouts + 1, immediates));
                    }
                });
                timer_call(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::TimerClearImmediate) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(id)
                if (13..=20).contains(&id)
                    || (24..=26).contains(&id)
                    || (33..=38).contains(&id) =>
            {
                assertion_call(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::AssertRejects) => {
                assert_rejects_call(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::AssertDoesNotReject) => {
                assert_rejects_call(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::ErrorsDetermineSpecificType) => {
                determine_specific_type(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::ProcessGetBuiltinModule) => {
                process_get_builtin_module(arguments)
            }
            HostCapabilityKind::Custom(404) => self.http_call(
                HostCapabilityKind::Custom(CapabilityName::HttpRequest),
                receiver,
                arguments,
            ),
            HostCapabilityKind::Custom(401) => self.http_call(
                HostCapabilityKind::Custom(CapabilityName::HttpRequestOn),
                receiver,
                arguments,
            ),
            HostCapabilityKind::Custom(402) => self.http_call(
                HostCapabilityKind::Custom(CapabilityName::HttpRequestEnd),
                receiver,
                arguments,
            ),
            HostCapabilityKind::Custom(403) => self.http_call(
                HostCapabilityKind::Custom(CapabilityName::HttpRequestWrite),
                receiver,
                arguments,
            ),
            HostCapabilityKind::Custom(0x070D) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::Stream) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(id) if (400..600).contains(&id) => {
                self.http_call(capability.kind, receiver, arguments)
            }
            HostCapabilityKind::Custom(id) if (1000..2000).contains(&id) => {
                self.stream_call(id, receiver, arguments)
            }
            HostCapabilityKind::Custom(id) if id >= 100 => self.hash_call(id, receiver, arguments),
            _ => Err(VmError::NotCallable),
        }
    }

    fn construct(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        if let Some(result) = self.construct_stream(capability, arguments) {
            return result;
        }
        if let Some(result) = self.construct_c(capability, arguments) {
            return result;
        }
        if let Some(result) = self.construct_b(capability, arguments) {
            return result;
        }
        if let Some(result) = self.construct_a(capability, arguments) {
            return result;
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::Url) {
            if arguments.is_empty() {
                return Ok(Value::object(vec![
                    ("protocol".into(), Value::Null),
                    ("slashes".into(), Value::Null),
                    ("auth".into(), Value::Null),
                    ("host".into(), Value::Null),
                    ("port".into(), Value::Null),
                    ("hostname".into(), Value::Null),
                    ("hash".into(), Value::Null),
                    ("search".into(), Value::Null),
                    ("query".into(), Value::Null),
                    ("pathname".into(), Value::Null),
                    ("path".into(), Value::Null),
                    ("href".into(), Value::Null),
                ]));
            }
            let input = match arguments.first() {
                Some(Value::String(value)) => value.as_str(),
                _ => return Err(VmError::EvalError("URL expects a string".into())),
            };
            let parsed = match arguments.get(1) {
                Some(Value::String(base)) => {
                    url::Url::parse(base).and_then(|base| base.join(input))
                }
                _ => url::Url::parse(input),
            }
            .map_err(|error| VmError::EvalError(error.to_string()))?;
            let id = self.next_url.get();
            self.next_url.set(id.saturating_add(1));
            self.urls.borrow_mut().insert(id, parsed.to_string());
            let pairs = parsed
                .query()
                .map(|query| query_pairs(query))
                .unwrap_or_default();
            self.params_state.borrow_mut().insert(id, pairs);
            let object = url_object(&parsed, id)?;
            self.url_objects.borrow_mut().insert(id, object.clone());
            return Ok(object);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::UrlSearchParams) {
            return url_search_params_construct(self, arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::EventEmitter) {
            let id = self.next_event.get();
            self.next_event.set(id.saturating_add(10));
            self.event_max.borrow_mut().insert(id, 10.0);
            let mut emitter = quench_runtime::host_api::object(vec![
                ("_events".into(), quench_runtime::host_api::object(vec![])),
                (
                    "emit".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildEmit)),
                ),
                (
                    "setMaxListeners".into(),
                    capability_function(HostCapabilityKind::Custom(id + 5)),
                ),
                (
                    "getMaxListeners".into(),
                    capability_function(HostCapabilityKind::Custom(id + 6)),
                ),
            ]);
            emitter = quench_runtime::execute::set_property(
                emitter,
                "captureRejections",
                Value::Boolean(false),
            );
            emitter = quench_runtime::execute::set_property(
                emitter,
                "asyncResource",
                quench_runtime::host_api::object(vec![(
                    "triggerAsyncId".into(),
                    capability_function(HostCapabilityKind::Custom(id + 7)),
                )]),
            );
            return Ok(emitter);
        }
        Err(VmError::NotCallable)
    }
}

impl QuenchNodeHost {}
