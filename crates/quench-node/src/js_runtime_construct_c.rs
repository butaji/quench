impl QuenchNodeHost {
    fn construct_c(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::ZlibGzip) {
            return self.zlib_stream(CapabilityName::ZlibCreateGzip);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::VmScript) {
            let source = arguments.first().map(safe_value_string).unwrap_or_default();
            if let Some(options) = arguments.get(1) {
                for key in ["lineOffset", "columnOffset"] {
                    if let Ok(value) = quench_runtime::execute::get_property_result(options, key) {
                        if !matches!(value, Value::Undefined) {
                            let valid = matches!(value, Value::Number(number)
                                if number.is_finite()
                                    && number.fract() == 0.0
                                    && (0.0..=u32::MAX as f64).contains(&number));
                            if !valid {
                                let code =
                                    if key == "columnOffset" && matches!(value, Value::Number(_)) {
                                        "ERR_OUT_OF_RANGE"
                                    } else {
                                        "ERR_INVALID_ARG_TYPE"
                                    };
                                return Err(VmError::Thrown(fs_error(
                                    code,
                                    "invalid script option",
                                )));
                            }
                        }
                    }
                }
            }
            let source_map = source
                .lines()
                .rev()
                .find_map(|line| line.trim().strip_prefix("//# sourceMappingURL="))
                .map(|value| Value::String(value.into()))
                .unwrap_or(Value::Undefined);
            VM_SCRIPT_CACHE_SOURCE.with(|stored| stored.replace(Some(source.clone())));
            let cached_data = quench_runtime::host_api::bytes(source.as_bytes());
            let produce_cached = arguments
                .get(1)
                .map(|options| {
                    matches!(
                        quench_runtime::execute::get_property_result(options, "produceCachedData"),
                        Ok(Value::Boolean(true))
                    )
                })
                .unwrap_or(false);
            let cached_rejected = arguments
                .get(1)
                .and_then(|options| {
                    match quench_runtime::execute::get_property_result(options, "cachedData") {
                        Ok(Value::Uint8Array(data)) => Some(Value::Boolean(
                            data.buffer.bytes.borrow().as_slice() != source.as_bytes(),
                        )),
                        _ => None,
                    }
                })
                .unwrap_or(Value::Boolean(false));
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "runInContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmScriptRunInContext,
                    )),
                ),
                (
                    "runInNewContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmScriptRunInNewContext,
                    )),
                ),
                (
                    "createCachedData".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmScriptCreateCachedData,
                    )),
                ),
                ("sourceMapURL".into(), source_map),
                ("cachedDataProduced".into(), Value::Boolean(produce_cached)),
                ("cachedData".into(), cached_data),
                ("cachedDataRejected".into(), cached_rejected),
            ]));
        }
            Err(VmError::EvalError(DISPATCH_UNHANDLED.into()))
        })();
        match result {
            Err(VmError::EvalError(message)) if message == DISPATCH_UNHANDLED => None,
            result => Some(result),
        }
    }
}
