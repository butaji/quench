impl QuenchNodeHost {
    fn dispatch_tmpdir(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
                HostCapabilityKind::Custom(CapabilityName::TmpdirRefresh) => {
                    let base = tmpdir_base();
                    let _ = std::fs::create_dir_all(&base);
                    Ok(Value::Undefined)
                }
                HostCapabilityKind::Custom(CapabilityName::TmpdirResolve) => {
                    let name = arguments.first().map(safe_value_string).unwrap_or_default();
                    Ok(Value::String(format!("{}/{}", tmpdir_base(), name)))
                }
                HostCapabilityKind::Custom(CapabilityName::TmpdirHasEnoughSpace) => {
                    Ok(Value::Boolean(false))
                }
                HostCapabilityKind::Custom(CapabilityName::TmpdirFileUrl) => {
                    let name = arguments.first().map(safe_value_string).unwrap_or_default();
                    Ok(quench_runtime::host_api::object(vec![(
                        "href".into(),
                        Value::String(format!("file://{name}")),
                    )]))
                }
                HostCapabilityKind::Custom(CapabilityName::CommonFsNextdir) => {
                    let name = match arguments.first() {
                        Some(_) => arguments.first().map(safe_value_string).unwrap_or_default(),
                        None => format!(
                            "copy_{}",
                            NODE_COPY_SEQUENCE.with(|seq| {
                                let n = seq.get();
                                seq.set(n + 1);
                                n
                            })
                        ),
                    };
                    Ok(Value::String(format!("{}/{}", tmpdir_base(), name)))
                }
                HostCapabilityKind::Custom(CapabilityName::CommonFsAssertDirEquivalent) => {
                    let dir1 = arguments.first().map(safe_value_string).unwrap_or_default();
                    let dir2 = arguments.get(1).map(safe_value_string).unwrap_or_default();
                    assert_dir_equivalent(&dir1, &dir2)?;
                    Ok(Value::Undefined)
                }
                HostCapabilityKind::Custom(CapabilityName::CommonMustNotMutateObjectDeep) => {
                    Ok(arguments.first().cloned().unwrap_or(Value::Undefined))
                }
                HostCapabilityKind::Custom(CapabilityName::CommonFsCollectEntries) => {
                    common_fs_collect_entries(arguments)
                }
                HostCapabilityKind::Custom(CapabilityName::CommonFsEntryIsDirectory) => {
                    common_fs_entry_is_directory(receiver)
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
