impl QuenchNodeHost {
    fn dispatch_buffer(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::BufferToString) => {
                buffer_to_string(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferConcat) => buffer_concat(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferEquals) => {
                buffer_equals(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferWrite) => {
                buffer_write(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIncludes) => {
                buffer_includes(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathParse) => path_parse(arguments, false),
            HostCapabilityKind::Custom(CapabilityName::PathFormat) => path_format(arguments, false),
            HostCapabilityKind::Custom(CapabilityName::PathWinParse) => path_parse(arguments, true),
            HostCapabilityKind::Custom(CapabilityName::PathWinFormat) => {
                path_format(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinBasename) => {
                path_win_basename(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinIsAbsolute) => {
                path_is_absolute_win(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathMatchesGlob) => {
                path_matches_glob(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinMatchesGlob) => {
                path_matches_glob(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::PathResolve) => {
                path_resolve(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinResolve) => {
                path_resolve(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIndexOf) => {
                buffer_search(receiver, arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferLastIndexOf) => {
                buffer_search(receiver, arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferToJson) => buffer_to_json(receiver),
            HostCapabilityKind::Custom(CapabilityName::BufferOf) => buffer_of(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafeSlow) => {
                buffer_alloc_unsafe(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafe) => {
                buffer_alloc_unsafe(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIsEncoding) => {
                buffer_is_encoding(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferSwap16) => buffer_swap(receiver, 2),
            HostCapabilityKind::Custom(CapabilityName::BufferSwap32) => buffer_swap(receiver, 4),
            HostCapabilityKind::Custom(CapabilityName::BufferSwap64) => buffer_swap(receiver, 8),
            HostCapabilityKind::Custom(CapabilityName::BufferCopyBytesFrom) => {
                buffer_copy_bytes_from(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigInt64LE) => {
                buffer_bigint(receiver, arguments, false, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigUInt64BE) => {
                buffer_bigint(receiver, arguments, true, false)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigInt64LE) => {
                buffer_bigint(receiver, arguments, false, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigUInt64BE) => {
                buffer_bigint(receiver, arguments, true, false)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderConstructor) => {
                string_decoder_constructor(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderWrite) => {
                string_decoder_write(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderEnd) => {
                string_decoder_end(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderText) => {
                string_decoder_text(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StringDecoderCall) => {
                let target = arguments.first().ok_or(VmError::NotCallable)?;
                string_decoder_constructor(Some(target), &arguments[1..])
            }
                _ => Err(VmError::EvalError(DISPATCH_UNHANDLED.into())),
            }
        })();
        match result {
            Err(VmError::EvalError(message)) if message == DISPATCH_UNHANDLED => None,
            result => Some(result),
        }
    }
}
