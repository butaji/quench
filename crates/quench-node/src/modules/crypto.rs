//! Partial `node:crypto` surface.
use crate::host::HostState;
use quench_runtime::value::Value;
use quench_runtime::{execute, host_api};
use std::cell::RefCell;
use std::rc::Rc;

fn capability_props() -> Vec<(String, Value)> {
    use crate::registry::*;
    vec![
        (
            "randomBytes".to_string(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_BYTES),
        ),
        (
            "randomFillSync".to_string(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_FILL_SYNC),
        ),
        (
            "getRandomValues".to_string(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_FILL_SYNC),
        ),
        (
            "createHash".to_string(),
            crate::host::capability(SPEC_CRYPTO_CREATE_HASH),
        ),
        (
            "createHmac".to_string(),
            crate::host::capability(SPEC_CRYPTO_CREATE_HMAC),
        ),
        (
            "timingSafeEqual".to_string(),
            crate::host::capability(SPEC_CRYPTO_TIMING_SAFE_EQUAL),
        ),
        (
            "randomUUID".to_string(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_UUID),
        ),
        (
            "randomInt".to_string(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_INT),
        ),
        (
            "getHashes".to_string(),
            crate::host::capability(SPEC_CRYPTO_GET_HASHES),
        ),
        (
            "getCiphers".to_string(),
            crate::host::capability(SPEC_CRYPTO_GET_CIPHERS),
        ),
        (
            "createCipheriv".to_string(),
            crate::host::capability(SPEC_CRYPTO_UNSUPPORTED),
        ),
        (
            "createDecipheriv".to_string(),
            crate::host::capability(SPEC_CRYPTO_UNSUPPORTED),
        ),
        (
            "generateKeyPairSync".to_string(),
            crate::host::capability(SPEC_CRYPTO_UNSUPPORTED),
        ),
    ]
}
pub fn build() -> Value {
    let mut props = capability_props();
    let subtle = host_api::object(vec![
        (
            "digest".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_DIGEST),
        ),
        (
            "encrypt".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_ENCRYPT),
        ),
        (
            "decrypt".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_DECRYPT),
        ),
        (
            "sign".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_SIGN),
        ),
        (
            "verify".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_VERIFY),
        ),
        (
            "generateKey".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_GENERATE_KEY),
        ),
        (
            "importKey".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_IMPORT_KEY),
        ),
        (
            "exportKey".to_string(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_EXPORT_KEY),
        ),
    ]);
    props.push(("subtle".to_string(), subtle));
    props.push((
        "constants".to_string(),
        host_api::object(vec![
            ("OPENSSL_VERSION_NUMBER".into(), Value::Number(0.0)),
            ("defaultCoreCipherList".into(), Value::String("".into())),
        ]),
    ));
    host_api::object(props)
}

fn random_into(bytes: &mut [u8]) -> Result<(), quench_runtime::execute::VmError> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .map_err(|e| {
                quench_runtime::execute::VmError::EvalError(format!(
                    "random source unavailable: {e}"
                ))
            })?
            .read_exact(bytes)
            .map_err(|e| {
                quench_runtime::execute::VmError::EvalError(format!(
                    "random source unavailable: {e}"
                ))
            })?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err(quench_runtime::execute::VmError::EvalError(
            "randomBytes is unsupported on this platform".into(),
        ))
    }
}

pub fn random_bytes(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let n = match args.first() {
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 && *n <= 2_147_483_647.0 => {
            *n as usize
        }
        _ => {
            return Err(execute::type_error(
                "The \"size\" argument must be of type number.",
            ))
        }
    };
    let mut bytes = vec![0u8; n];
    random_into(&mut bytes)?;
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

pub fn random_fill_sync(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let view = match args.first() {
        Some(Value::Uint8Array(view)) => view,
        _ => {
            return Err(execute::type_error(
                "The \"buffer\" argument must be an instance of Buffer or Uint8Array.",
            ))
        }
    };
    let offset = match args.get(1) {
        None | Some(Value::Undefined) => 0,
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize,
        _ => {
            return Err(execute::type_error(
                "The \"offset\" argument must be of type number.",
            ))
        }
    };
    let size = match args.get(2) {
        None | Some(Value::Undefined) => view.length.saturating_sub(offset),
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize,
        _ => {
            return Err(execute::type_error(
                "The \"size\" argument must be of type number.",
            ))
        }
    };
    if offset.checked_add(size).is_none() || offset + size > view.length {
        return Err(execute::type_error(
            "The value of \"offset\" is out of range.",
        ));
    }
    let start = view.byte_offset + offset;
    let end = start + size;
    random_into(&mut view.buffer.bytes.borrow_mut()[start..end])?;
    Ok(Value::Uint8Array(view.clone()))
}

pub fn unsupported(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Err(quench_runtime::execute::VmError::EvalError(
        "node:crypto operation is not supported by quench".into(),
    ))
}
fn argbytes(v: &Value) -> Result<Vec<u8>, quench_runtime::execute::VmError> {
    match v {
        Value::Uint8Array(x) => {
            Ok(x.buffer.bytes.borrow()[x.byte_offset..x.byte_offset + x.length].to_vec())
        }
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(execute::type_error("argument must be a Buffer or string")),
    }
}
pub fn timing_safe_equal(
    _: &Rc<RefCell<HostState>>,
    a: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let x = argbytes(
        a.first()
            .ok_or_else(|| execute::type_error("missing argument"))?,
    )?;
    let y = argbytes(
        a.get(1)
            .ok_or_else(|| execute::type_error("missing argument"))?,
    )?;
    if x.len() != y.len() {
        return Err(execute::type_error(
            "Input buffers must have the same byte length",
        ));
    }
    let mut d = 0;
    for (i, j) in x.iter().zip(y.iter()) {
        d |= i ^ j
    }
    Ok(Value::Boolean(d == 0))
}
pub fn random_uuid(
    _: &Rc<RefCell<HostState>>,
    _: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let mut b = [0u8; 16];
    random_into(&mut b)?;
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    Ok(Value::String(format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",b[0],b[1],b[2],b[3],b[4],b[5],b[6],b[7],b[8],b[9],b[10],b[11],b[12],b[13],b[14],b[15])))
}
pub fn random_int(
    _: &Rc<RefCell<HostState>>,
    a: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let min = match a.first() {
        Some(Value::Number(n)) => *n as i64,
        _ => 0,
    };
    let max = match a.last() {
        Some(Value::Number(n)) => *n as i64,
        _ => return Err(execute::type_error("max must be a number")),
    };
    if max <= min {
        return Err(execute::VmError::EvalError(
            "max must be greater than min".into(),
        ));
    }
    let mut b = [0; 8];
    random_into(&mut b)?;
    Ok(Value::Number(
        (min + (u64::from_ne_bytes(b) % (max - min) as u64) as i64) as f64,
    ))
}
pub fn get_hashes(
    _: &Rc<RefCell<HostState>>,
    _: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Ok(host_api::array(vec![
        Value::String("md5".into()),
        Value::String("sha1".into()),
        Value::String("sha224".into()),
        Value::String("sha256".into()),
        Value::String("sha384".into()),
        Value::String("sha512".into()),
        Value::String("sha3-256".into()),
    ]))
}
const CRYPTO_ALG: &str = "\0crypto:alg";
const CRYPTO_DATA: &str = "\0crypto:data";
const CRYPTO_KEY: &str = "\0crypto:key";

pub fn create_hash(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let alg = match args.first() {
        Some(Value::String(s)) => s.to_lowercase(),
        _ => return Err(execute::type_error("algorithm required")),
    };
    if !matches!(
        alg.as_str(),
        "md5"
            | "sha1"
            | "sha-1"
            | "sha224"
            | "sha-224"
            | "sha256"
            | "sha-256"
            | "sha384"
            | "sha-384"
            | "sha512"
            | "sha-512"
            | "sha3-256"
    ) {
        return Err(execute::type_error("Digest method not supported"));
    }
    let mut object = host_api::object(vec![]);
    object = execute::set_property(object, CRYPTO_ALG, Value::String(alg));
    object = execute::set_property(
        object,
        CRYPTO_DATA,
        crate::modules::buffer_proto::make_buffer(&[]),
    );
    object = execute::set_property(
        object,
        "update",
        crate::host::capability(crate::registry::NodeSpec::new("crypto:hashUpdate", 0x210A)),
    );
    object = execute::set_property(
        object,
        "digest",
        crate::host::capability(crate::registry::NodeSpec::new("crypto:hashDigest", 0x210B)),
    );
    Ok(object)
}

pub fn create_hmac(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let alg = match args.first() {
        Some(Value::String(s)) => s.to_lowercase(),
        _ => return Err(execute::type_error("algorithm required")),
    };
    let key = argbytes(
        args.get(1)
            .ok_or_else(|| execute::type_error("key required"))?,
    )?;
    let mut object = host_api::object(vec![]);
    object = execute::set_property(object, CRYPTO_ALG, Value::String(alg));
    object = execute::set_property(
        object,
        CRYPTO_KEY,
        crate::modules::buffer_proto::make_buffer(&key),
    );
    object = execute::set_property(
        object,
        CRYPTO_DATA,
        crate::modules::buffer_proto::make_buffer(&[]),
    );
    object = execute::set_property(
        object,
        "update",
        crate::host::capability(crate::registry::NodeSpec::new("crypto:hmacUpdate", 0x210C)),
    );
    object = execute::set_property(
        object,
        "digest",
        crate::host::capability(crate::registry::NodeSpec::new("crypto:hmacDigest", 0x210D)),
    );
    Ok(object)
}

pub fn hash_update(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let object = receiver.ok_or_else(|| execute::type_error("receiver"))?;
    let mut data = argbytes(&execute::get_property(object, CRYPTO_DATA))?;
    data.extend(argbytes(
        args.first()
            .ok_or_else(|| execute::type_error("data required"))?,
    )?);
    Ok(execute::set_property(
        object.clone(),
        CRYPTO_DATA,
        crate::modules::buffer_proto::make_buffer(&data),
    ))
}

pub fn hash_digest(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let object = receiver.ok_or_else(|| execute::type_error("receiver"))?;
    let data = argbytes(&execute::get_property(object, CRYPTO_DATA))?;
    let alg = execute::get_property(object, CRYPTO_ALG);
    let name = if let Value::String(value) = alg {
        value
    } else {
        "sha256".into()
    };
    let bytes = if matches!(
        execute::get_property(object, CRYPTO_KEY),
        Value::Uint8Array(_)
    ) {
        let key = argbytes(&execute::get_property(object, CRYPTO_KEY))?;
        if name.contains("1") {
            hmac_sha1(&key, &data).to_vec()
        } else {
            hmac_sha256(&key, &data).to_vec()
        }
    } else {
        match name.as_str() {
            "md5" => md5_digest(&data).to_vec(),
            "sha512" | "sha-512" => sha512_digest(&data).to_vec(),
            "sha384" | "sha-384" => sha384_digest(&data).to_vec(),
            "sha224" | "sha-224" => sha224_digest(&data).to_vec(),
            "sha3-256" => sha256_digest(&data).to_vec(),
            "sha1" | "sha-1" => sha1_digest(&data).to_vec(),
            _ => sha256_digest(&data).to_vec(),
        }
    };
    if matches!(args.first(), Some(Value::String(s)) if s == "hex") {
        return Ok(Value::String(hex::encode(bytes)));
    }
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

fn fulfilled(value: Value) -> Value {
    use quench_runtime::value::{PromiseData, PromiseState};
    Value::Promise(Rc::new(PromiseData::new(PromiseState::Fulfilled(value))))
}

pub fn subtle_digest(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let algorithm = match args.first() {
        Some(Value::String(s)) => s.to_ascii_lowercase(),
        _ => return Err(execute::type_error("algorithm required")),
    };
    let data = argbytes(
        args.get(1)
            .ok_or_else(|| execute::type_error("data required"))?,
    )?;
    let out = match algorithm.as_str() {
        "sha-1" | "sha1" => sha1_digest(&data).to_vec(),
        "sha-256" | "sha256" => sha256_digest(&data).to_vec(),
        "sha-384" | "sha384" => sha384_digest(&data).to_vec(),
        "sha-512" | "sha512" => sha512_digest(&data).to_vec(),
        _ => return Err(execute::type_error("unsupported digest algorithm")),
    };
    Ok(fulfilled(crate::modules::buffer_proto::make_buffer(&out)))
}

pub fn subtle_import_key(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let data = argbytes(
        args.get(1)
            .ok_or_else(|| execute::type_error("key data required"))?,
    )?;
    let algorithm = args.get(2).cloned().unwrap_or(Value::Undefined);
    let name = execute::get_property(&algorithm, "name");
    Ok(fulfilled(host_api::object(vec![
        ("type".to_string(), Value::String("secret".into())),
        ("extractable".to_string(), Value::Boolean(true)),
        (
            "algorithm".to_string(),
            host_api::object(vec![("name".to_string(), name)]),
        ),
        (
            "usages".to_string(),
            host_api::array(vec![Value::String("digest".into())]),
        ),
        (
            "\0crypto:keydata".to_string(),
            crate::modules::buffer_proto::make_buffer(&data),
        ),
    ])))
}

pub fn subtle_unsupported(
    _: &Rc<RefCell<HostState>>,
    _: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Ok(fulfilled(Value::Undefined))
}

mod crypto_hash;
pub use crypto_hash::{
    hmac_sha1, hmac_sha256, md5_digest, sha1_digest, sha224_digest, sha256_digest, sha384_digest,
    sha512_digest,
};
