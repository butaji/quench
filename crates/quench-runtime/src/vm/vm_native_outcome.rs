// One typed result crosses both native bridge boundaries. Status validates the
// representation; it never selects independently stored transition/error data.

enum NativeBridgeOutcome {
    Transition(DispatchTransition),
    Throw { pc: usize, error: VmError },
}

fn finish_native_outcome(
    status: u64,
    outcome: Option<NativeBridgeOutcome>,
    entry_started: bool,
    boundary: &'static str,
) -> Result<DispatchTransition, crate::machine::NativeDispatchError> {
    let status = NativeStatus::from(status);
    match (status, outcome) {
        (NativeStatus::Ok, Some(NativeBridgeOutcome::Transition(result))) => Ok(result),
        (NativeStatus::SemanticError, Some(NativeBridgeOutcome::Throw { pc, error })) => {
            Err(crate::machine::NativeDispatchError::SemanticAt { pc, error })
        }
        _ => Err(malformed_native_outcome(status, entry_started, boundary)),
    }
}

fn malformed_native_outcome(
    status: NativeStatus,
    entry_started: bool,
    boundary: &'static str,
) -> crate::machine::NativeDispatchError {
    let message = match (entry_started, status) {
        (true, NativeStatus::Interrupt) => "interrupted after committed progress".to_owned(),
        (true, NativeStatus::Unknown(_)) => "returned an invalid post-entry status".to_owned(),
        (true, _) => "reported a malformed post-entry outcome".to_owned(),
        (false, NativeStatus::CommittedError) => "reported a pre-entry failure".to_owned(),
        (false, NativeStatus::Unknown(_)) => "returned an invalid entry status".to_owned(),
        (false, _) => "rejected before entry".to_owned(),
    };
    let message = format!("native {boundary} {message}");
    if entry_started {
        crate::machine::NativeDispatchError::Committed(message)
    } else {
        crate::machine::NativeDispatchError::Physical(message)
    }
}
