impl QuenchNodeHost {
    fn construct_a(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::UrlSearchParams) {
            return url_search_params_construct(self, arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::UrlPattern) {
            return url_pattern_construct(arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::ReplServer) {
            let options = arguments.first();
            let colors = options
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "useColors").ok()
                })
                .is_some_and(|value| matches!(value, Value::Boolean(true)));
            if let Some(output) = options.and_then(|value| {
                quench_runtime::execute::get_property_result(value, "output").ok()
            }) {
                if let Ok(write) = quench_runtime::execute::get_property_result(&output, "write") {
                    let _ = quench_runtime::execute::call(
                        &write,
                        &output,
                        &[Value::String("\"'string'\"".into())],
                    );
                }
            }
            let options =
                quench_runtime::host_api::object(vec![("colors".into(), Value::Boolean(colors))]);
            let writer = quench_runtime::host_api::object(vec![("options".into(), options)]);
            return Ok(quench_runtime::host_api::object(vec![(
                "writer".into(),
                writer,
            )]));
        }
        if capability.kind
            == HostCapabilityKind::Custom(CapabilityName::CryptoCertificateConstructor)
        {
            return Ok(Value::object(vec![
                (
                    "verifySpkac".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCertificateVerifySpkac,
                    )),
                ),
                (
                    "exportPublicKey".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCertificateExportPublicKey,
                    )),
                ),
                (
                    "exportChallenge".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CryptoCertificateExportChallenge,
                    )),
                ),
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
