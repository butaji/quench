//! Partial `node:crypto` surface.
use std::cell::RefCell;
use std::rc::Rc;
use quench_runtime::{execute, host_api};
use quench_runtime::value::Value;
use crate::host::HostState;

pub fn build() -> Value {
    host_api::object(vec![
        ("randomBytes".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES)),
        ("randomFillSync".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_FILL_SYNC)),
        ("createHash".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HASH)),
        ("createHmac".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HMAC)),
        ("timingSafeEqual".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_TIMING_SAFE_EQUAL)),
        ("randomUUID".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_UUID)),
        ("randomInt".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_INT)),
        ("getHashes".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_GET_HASHES)),
        ("getCiphers".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_GET_CIPHERS)),
        ("constants".into(), host_api::object(vec![
            ("OPENSSL_VERSION_NUMBER".into(), Value::Number(0.0)),
            ("defaultCoreCipherList".into(), Value::String("".into())),
        ])),
        ("createCipheriv".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
        ("createDecipheriv".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
        ("generateKeyPairSync".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
    ])
}

fn random_into(bytes: &mut [u8]) -> Result<(), quench_runtime::execute::VmError> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .map_err(|e| quench_runtime::execute::VmError::EvalError(format!("random source unavailable: {e}")))?
            .read_exact(bytes)
            .map_err(|e| quench_runtime::execute::VmError::EvalError(format!("random source unavailable: {e}")))?;
        Ok(())
    }
    #[cfg(not(unix))]
    { Err(quench_runtime::execute::VmError::EvalError("randomBytes is unsupported on this platform".into())) }
}


pub fn random_bytes(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let n = match args.first() {
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 && *n <= 2_147_483_647.0 => *n as usize,
        _ => return Err(execute::type_error("The \"size\" argument must be of type number.")),
    };
    let mut bytes = vec![0u8; n];
    random_into(&mut bytes)?;
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

pub fn random_fill_sync(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let view = match args.first() {
        Some(Value::Uint8Array(view)) => view,
        _ => return Err(execute::type_error("The \"buffer\" argument must be an instance of Buffer or Uint8Array.")),
    };
    let offset = match args.get(1) {
        None | Some(Value::Undefined) => 0,
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize,
        _ => return Err(execute::type_error("The \"offset\" argument must be of type number.")),
    };
    let size = match args.get(2) {
        None | Some(Value::Undefined) => view.length.saturating_sub(offset),
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize,
        _ => return Err(execute::type_error("The \"size\" argument must be of type number.")),
    };
    if offset.checked_add(size).is_none() || offset + size > view.length {
        return Err(execute::type_error("The value of \"offset\" is out of range."));
    }
    let start = view.byte_offset + offset;
    let end = start + size;
    random_into(&mut view.buffer.bytes.borrow_mut()[start..end])?;
    Ok(Value::Uint8Array(view.clone()))
}

pub fn unsupported(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    Err(quench_runtime::execute::VmError::EvalError("node:crypto operation is not supported by quench".into()))
}
