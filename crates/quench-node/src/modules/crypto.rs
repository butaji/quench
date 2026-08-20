//! Partial `node:crypto` surface.
use std::cell::RefCell;
use std::rc::Rc;
use quench_runtime::{execute, host_api};
use quench_runtime::value::Value;
use crate::host::HostState;

pub fn build() -> Value {
    host_api::object(vec![
        ("randomBytes".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES)),
        ("randomFillSync".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES)),
        ("constants".into(), host_api::object(vec![
            ("OPENSSL_VERSION_NUMBER".into(), Value::Number(0.0)),
            ("defaultCoreCipherList".into(), Value::String("".into())),
        ])),
        ("createHash".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
        ("createCipheriv".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
        ("createDecipheriv".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
        ("generateKeyPairSync".into(), crate::host::capability(crate::registry::SPEC_CRYPTO_UNSUPPORTED)),
    ])
}

pub fn random_bytes(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    let n = match args.first() { Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 && *n <= 2_147_483_647.0 => *n as usize, _ => return Err(execute::type_error("The \"size\" argument must be of type number.")) };
    let mut bytes = vec![0u8; n];
    #[cfg(unix)] { use std::io::Read; std::fs::File::open("/dev/urandom").map_err(|e| quench_runtime::execute::VmError::EvalError(format!("random source unavailable: {e}")) )?.read_exact(&mut bytes).map_err(|e| quench_runtime::execute::VmError::EvalError(format!("random source unavailable: {e}")) )?; }
    #[cfg(not(unix))] { return Err(quench_runtime::execute::VmError::EvalError("randomBytes is unsupported on this platform".into())); }
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

pub fn unsupported(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    Err(quench_runtime::execute::VmError::EvalError("node:crypto operation is not supported by quench".into()))
}
