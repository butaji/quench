fn query_pairs(input: &str) -> Vec<(String, String)> {
    input
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.find('=') {
            Some(index) => (
                pair[..index].to_string(),
                pair[index + 1..].to_string(),
            ),
            None => (pair.to_string(), String::new()),
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
            HostCapabilityKind::Custom(CapabilityName::ProcessNextTick) => next_tick(arguments),
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
                    || (33..=35).contains(&id) =>
            {
                assertion_call(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpServer | CapabilityName::HttpGet) => {
                self.http_call(capability.kind, arguments)
            }
            HostCapabilityKind::Custom(id) if (400..600).contains(&id) => {
                self.http_call(capability.kind, arguments)
            }
            HostCapabilityKind::Custom(id) if (200..300).contains(&id) => {
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
