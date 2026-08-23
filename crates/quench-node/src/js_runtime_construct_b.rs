impl QuenchNodeHost {
    fn construct_b(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::BufferFrom) {
            if matches!(arguments.first(), Some(Value::Number(_))) {
                if arguments.len() > 1 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        &format!(
                            "The \"string\" argument must be of type string. Received type number ({})",
                            safe_value_string(arguments.first().unwrap()),
                        ),
                    )));
                }
                return buffer_alloc(arguments);
            }
            return buffer_from(arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::StringDecoderConstructor) {
            return string_decoder_constructor(None, arguments);
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::TextEncoderConstructor) {
            return text_encoder_constructor();
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::TextDecoderConstructor) {
            return text_decoder_constructor();
        }
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::WorkerConstructor) {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "on".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::WorkerOn)),
                ),
                (
                    "once".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::WorkerOnce)),
                ),
                (
                    "postMessage".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::WorkerPostMessage,
                    )),
                ),
                (
                    "terminate".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::WorkerTerminate,
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
