//! Partial `node:crypto` surface.
mod crypto_subtle;
use crate::host::HostState;
use quench_runtime::value::Value;
use quench_runtime::{execute, host_api};
use std::cell::RefCell;
use std::rc::Rc;

fn capability_props_core() -> Vec<(String, Value)> {
    use crate::registry::*;
    vec![
        (
            "randomBytes".into(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_BYTES),
        ),
        (
            "randomFillSync".into(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_FILL_SYNC),
        ),
        (
            "getRandomValues".into(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_FILL_SYNC),
        ),
        (
            "createHash".into(),
            crate::host::capability(SPEC_CRYPTO_CREATE_HASH),
        ),
        (
            "createHmac".into(),
            crate::host::capability(SPEC_CRYPTO_CREATE_HMAC),
        ),
        (
            "timingSafeEqual".into(),
            crate::host::capability(SPEC_CRYPTO_TIMING_SAFE_EQUAL),
        ),
        (
            "randomUUID".into(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_UUID),
        ),
        (
            "randomInt".into(),
            crate::host::capability(SPEC_CRYPTO_RANDOM_INT),
        ),
    ]
}

fn capability_props() -> Vec<(String, Value)> {
    use crate::registry::*;
    let mut props = capability_props_core();
    props.extend(
        [
            ("getCiphers", SPEC_CRYPTO_GET_CIPHERS),
            ("createCipheriv", SPEC_CRYPTO_UNSUPPORTED),
            ("createDecipheriv", SPEC_CRYPTO_UNSUPPORTED),
            ("generateKeyPairSync", SPEC_CRYPTO_UNSUPPORTED),
        ]
        .into_iter()
        .map(|(name, spec)| (name.into(), crate::host::capability(spec))),
    );
    props
}
pub fn build() -> Value {
    let mut props = capability_props();
    let subtle = crypto_subtle::subtle_object();
    props.push(("subtle".to_string(), subtle.clone()));
    // Node exposes the same WebCrypto namespace through `crypto.webcrypto`.
    props.push((
        "webcrypto".to_string(),
        host_api::object(vec![("subtle".to_string(), subtle)]),
    ));
    props.push((
        "constants".to_string(),
        host_api::object(vec![
            ("OPENSSL_VERSION_NUMBER".into(), Value::Number(0.0)),
            ("defaultCoreCipherList".into(), Value::String("".into())),
        ]),
    ));
    host_api::object(props)
}
pub(crate) fn subtle() -> Value {
    crypto_subtle::subtle_object()
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
    let (buffer, offset, length) = match v {
        Value::Uint8Array(view) => (&view.buffer, view.byte_offset, view.byte_length()),
        Value::DataView(view) => {
            if *view.buffer.detached.borrow() || view.is_out_of_bounds() {
                return Err(execute::type_error("argument must be a Buffer or string"));
            }
            (&view.buffer, view.byte_offset, view.byte_length())
        }
        Value::ArrayBuffer(buffer) => {
            if *buffer.detached.borrow() {
                return Err(execute::type_error("argument must be a Buffer or string"));
            }
            return Ok(buffer.bytes.borrow().clone());
        }
        Value::String(s) => return Ok(s.as_bytes().to_vec()),
        _ => return Err(execute::type_error("argument must be a Buffer or string")),
    };
    let bytes = buffer.bytes.borrow();
    let end = offset
        .checked_add(length)
        .ok_or_else(|| execute::type_error("argument must be a Buffer or string"))?;
    bytes
        .get(offset..end)
        .map(|slice| slice.to_vec())
        .ok_or_else(|| execute::type_error("argument must be a Buffer or string"))
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
    // Node accepts randomInt(max) and randomInt(min, max), with both bounds
    // required to be safe integers and max exclusive.
    let number = |value: &Value| match value {
        Value::Number(n)
            if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 =>
        {
            Some(*n as i64)
        }
        _ => None,
    };
    let (min, max) = match a {
        [max] => (
            0,
            number(max).ok_or_else(|| execute::type_error("max must be a safe integer"))?,
        ),
        [min, max] => (
            number(min).ok_or_else(|| execute::type_error("min must be a safe integer"))?,
            number(max).ok_or_else(|| execute::type_error("max must be a safe integer"))?,
        ),
        _ => {
            return Err(execute::type_error(
                "randomInt requires one or two integer arguments",
            ))
        }
    };
    if max <= min {
        return Err(quench_runtime::execute::VmError::EvalError(
            "max must be greater than min".into(),
        ));
    }
    let range = (max - min) as u64;
    // Rejection sampling avoids modulo bias for ranges that do not divide
    // the 64-bit random space evenly.
    let cutoff = u64::MAX - (u64::MAX % range);
    let offset = loop {
        let mut b = [0; 8];
        random_into(&mut b)?;
        let sample = u64::from_ne_bytes(b);
        if sample < cutoff {
            break sample % range;
        }
    };
    Ok(Value::Number((min + offset as i64) as f64))
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
/// Return the cipher names implemented by the runtime's common crypto baseline.
///
/// Keep this list deliberately conservative: callers use it to decide whether
/// an algorithm can be requested, so advertising an algorithm without a real
/// implementation is worse than omitting it.
pub fn get_ciphers(
    _: &Rc<RefCell<HostState>>,
    _: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Ok(host_api::array(vec![Value::String("aes-256-gcm".into())]))
}

pub fn create_hash(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let alg = parse_hash_algorithm(args)?;
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

fn parse_hash_algorithm(args: &[Value]) -> Result<String, quench_runtime::execute::VmError> {
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
    Ok(alg)
}

pub fn create_hmac(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let alg = match args.first() {
        Some(Value::String(s)) => s.to_lowercase(),
        _ => return Err(execute::type_error("algorithm required")),
    };
    if !matches!(
        alg.as_str(),
        "sha1" | "sha-1" | "sha256" | "sha-256" | "sha384" | "sha-384" | "sha512" | "sha-512"
    ) {
        return Err(execute::type_error("Digest method not supported"));
    }
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
        match name.as_str() {
            "sha1" | "sha-1" => hmac_sha1(&key, &data).to_vec(),
            "sha384" | "sha-384" => hmac_sha384(&key, &data).to_vec(),
            "sha512" | "sha-512" => hmac_sha512(&key, &data).to_vec(),
            _ => hmac_sha256(&key, &data).to_vec(),
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
        Some(Value::Object(_)) | Some(Value::ObjectAlias(_)) => {
            match execute::get_property(args.first().unwrap(), "name") {
                Value::String(s) => s.to_ascii_lowercase(),
                _ => return Err(execute::type_error("algorithm name required")),
            }
        }
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
    // Preserve known sub-algorithm fields (e.g. `hash`) so downstream operations
    // (sign/verify) can dispatch on them.
    let hash_v = execute::get_property(&algorithm, "hash");
    let length_v = execute::get_property(&algorithm, "length");
    let mut alg_fields: Vec<(String, Value)> = vec![("name".to_string(), name)];
    if !matches!(hash_v, Value::Undefined) {
        alg_fields.push(("hash".to_string(), hash_v));
    }
    if !matches!(length_v, Value::Undefined) {
        alg_fields.push(("length".to_string(), length_v));
    }
    let extractable = match args.get(3) {
        Some(Value::Boolean(value)) => *value,
        _ => return Err(execute::type_error("extractable must be a boolean")),
    };
    let usages = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    Ok(fulfilled(host_api::object(vec![
        ("type".to_string(), Value::String("secret".into())),
        ("extractable".to_string(), Value::Boolean(extractable)),
        ("algorithm".to_string(), host_api::object(alg_fields)),
        ("usages".to_string(), usages),
        (
            "\0crypto:keydata".to_string(),
            crate::modules::buffer_proto::make_buffer(&data),
        ),
    ])))
}

fn crypto_key_hash_name(key: &Value) -> String {
    let algorithm = execute::get_property(key, "algorithm");
    let name = execute::get_property(&algorithm, "name");
    if let Value::String(s) = name {
        return s.to_string();
    }
    String::new()
}

pub fn subtle_sign(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let alg_name = match execute::get_property(&algorithm, "name") {
        Value::String(s) => s.to_string(),
        _ => return Err(execute::type_error("sign: algorithm.name required")),
    };
    if alg_name != "HMAC" {
        return Err(execute::type_error(
            format!("sign: unsupported algorithm {alg_name}").as_str(),
        ));
    }
    let hash_name = match execute::get_property(&algorithm, "hash") {
        Value::String(s) => s.to_string(),
        _ => "SHA-256".to_string(),
    };
    let key = args.get(1).cloned().unwrap_or(Value::Undefined);
    let key_hash = crypto_key_hash_name(&key);
    if !key_hash.is_empty() && key_hash != "HMAC" {
        return Err(execute::type_error("sign: key is not an HMAC key"));
    }
    let secret = crypto_key_bytes(&key)?;
    let data_v = args.get(2).cloned().unwrap_or(Value::Undefined);
    let data = argbytes(&data_v)?;
    let mac: Vec<u8> = match hash_name.as_str() {
        "SHA-256" => hmac_sha256(&secret, &data).to_vec(),
        "SHA-1" => hmac_sha1(&secret, &data).to_vec(),
        "SHA-384" => hmac_sha384(&secret, &data).to_vec(),
        "SHA-512" => hmac_sha512(&secret, &data).to_vec(),
        _ => {
            return Err(execute::type_error(
                format!("sign: unsupported hash {hash_name}").as_str(),
            ))
        }
    };
    Ok(fulfilled(crate::modules::buffer_proto::make_buffer(&mac)))
}

pub fn subtle_verify(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let alg_name = match execute::get_property(&algorithm, "name") {
        Value::String(s) => s.to_string(),
        _ => return Err(execute::type_error("verify: algorithm.name required")),
    };
    if alg_name != "HMAC" {
        return Err(execute::type_error(
            format!("verify: unsupported algorithm {alg_name}").as_str(),
        ));
    }
    let hash_name = match execute::get_property(&algorithm, "hash") {
        Value::String(s) => s.to_string(),
        _ => "SHA-256".to_string(),
    };
    let key = args.get(1).cloned().unwrap_or(Value::Undefined);
    let key_hash = crypto_key_hash_name(&key);
    if !key_hash.is_empty() && key_hash != "HMAC" {
        return Err(execute::type_error("verify: key is not an HMAC key"));
    }
    let secret = crypto_key_bytes(&key)?;
    let signature_v = args.get(2).cloned().unwrap_or(Value::Undefined);
    let signature = argbytes(&signature_v)?;
    let data_v = args.get(3).cloned().unwrap_or(Value::Undefined);
    let data = argbytes(&data_v)?;
    let mac: Vec<u8> = match hash_name.as_str() {
        "SHA-256" => hmac_sha256(&secret, &data).to_vec(),
        "SHA-1" => hmac_sha1(&secret, &data).to_vec(),
        "SHA-384" => hmac_sha384(&secret, &data).to_vec(),
        "SHA-512" => hmac_sha512(&secret, &data).to_vec(),
        _ => {
            return Err(execute::type_error(
                format!("verify: unsupported hash {hash_name}").as_str(),
            ))
        }
    };
    let ok = mac.len() == signature.len() && mac.iter().zip(signature.iter()).all(|(a, b)| a == b);
    Ok(fulfilled(Value::Boolean(ok)))
}

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes128Gcm, Aes256Gcm, Nonce,
};

fn aes_gcm_nonce(iv_v: &Value) -> Result<[u8; 12], quench_runtime::execute::VmError> {
    let iv = argbytes(iv_v)?;
    if iv.len() != 12 {
        return Err(execute::type_error("AES-GCM: iv must be 12 bytes"));
    }
    let mut n = [0u8; 12];
    n.copy_from_slice(&iv);
    Ok(n)
}

pub fn subtle_encrypt(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let alg_name = match execute::get_property(&algorithm, "name") {
        Value::String(s) => s.to_string(),
        _ => return Err(execute::type_error("encrypt: algorithm.name required")),
    };
    if alg_name != "AES-GCM" {
        return Err(execute::type_error(
            format!("encrypt: unsupported algorithm {alg_name}").as_str(),
        ));
    }
    let iv = aes_gcm_nonce(&execute::get_property(&algorithm, "iv"))?;
    let key = args.get(1).cloned().unwrap_or(Value::Undefined);
    let secret = crypto_key_bytes(&key)?;
    let data_v = args.get(2).cloned().unwrap_or(Value::Undefined);
    let plaintext = argbytes(&data_v)?;
    let ciphertext = match secret.len() {
        16 => Aes128Gcm::new_from_slice(&secret)
            .map_err(|e| execute::type_error(format!("AES-GCM key: {e}").as_str()))?
            .encrypt(Nonce::from_slice(&iv), plaintext.as_ref()),
        32 => Aes256Gcm::new_from_slice(&secret)
            .map_err(|e| execute::type_error(format!("AES-GCM key: {e}").as_str()))?
            .encrypt(Nonce::from_slice(&iv), plaintext.as_ref()),
        _ => return Err(execute::type_error("AES-GCM: key must be 16 or 32 bytes")),
    }
    .map_err(|e| execute::type_error(format!("AES-GCM encrypt: {e}").as_str()))?;
    Ok(fulfilled(crate::modules::buffer_proto::make_buffer(
        &ciphertext,
    )))
}

pub fn subtle_decrypt(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let alg_name = match execute::get_property(&algorithm, "name") {
        Value::String(s) => s.to_string(),
        _ => return Err(execute::type_error("decrypt: algorithm.name required")),
    };
    if alg_name != "AES-GCM" {
        return Err(execute::type_error(
            format!("decrypt: unsupported algorithm {alg_name}").as_str(),
        ));
    }
    let iv = aes_gcm_nonce(&execute::get_property(&algorithm, "iv"))?;
    let key = args.get(1).cloned().unwrap_or(Value::Undefined);
    let secret = crypto_key_bytes(&key)?;
    let data_v = args.get(2).cloned().unwrap_or(Value::Undefined);
    let ciphertext = argbytes(&data_v)?;
    let plaintext = match secret.len() {
        16 => Aes128Gcm::new_from_slice(&secret)
            .map_err(|e| execute::type_error(format!("AES-GCM key: {e}").as_str()))?
            .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref()),
        32 => Aes256Gcm::new_from_slice(&secret)
            .map_err(|e| execute::type_error(format!("AES-GCM key: {e}").as_str()))?
            .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref()),
        _ => return Err(execute::type_error("AES-GCM: key must be 16 or 32 bytes")),
    }
    .map_err(|e| execute::type_error(format!("AES-GCM decrypt: {e}").as_str()))?;
    Ok(fulfilled(crate::modules::buffer_proto::make_buffer(
        &plaintext,
    )))
}

pub fn subtle_export_key(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let format = match args.first().cloned().unwrap_or(Value::Undefined) {
        Value::String(s) => s.to_string(),
        _ => return Err(execute::type_error("exportKey: format required")),
    };
    if format != "raw" && format != "raw-secret" {
        return Err(execute::type_error(
            format!("exportKey: unsupported format {format}").as_str(),
        ));
    }
    let key = args.get(1).cloned().unwrap_or(Value::Undefined);
    let extractable = execute::get_property(&key, "extractable");
    if matches!(extractable, Value::Boolean(false)) {
        return Err(execute::type_error("exportKey: key is not extractable"));
    }
    let secret = crypto_key_bytes(&key)?;
    Ok(fulfilled(crate::modules::buffer_proto::make_buffer(
        &secret,
    )))
}

pub fn subtle_generate_key(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let alg_name = match execute::get_property(&algorithm, "name") {
        Value::String(s) => s.to_string(),
        _ => return Err(execute::type_error("generateKey: algorithm.name required")),
    };
    let length_bits = match execute::get_property(&algorithm, "length") {
        Value::Number(n) if n.is_finite() && n > 0.0 && n.fract() == 0.0 && n <= 4096.0 => {
            n as usize
        }
        _ => {
            return Err(execute::type_error(
                "generateKey: algorithm.length required",
            ))
        }
    };
    if length_bits % 8 != 0 {
        return Err(execute::type_error(
            "generateKey: algorithm.length must be a multiple of 8",
        ));
    }
    let byte_length = length_bits / 8;
    let mut buf = vec![0u8; byte_length];
    random_into(&mut buf).map_err(|_e: quench_runtime::execute::VmError| {
        execute::type_error("generateKey: random failed")
    })?;
    let extractable = args.get(1).cloned().unwrap_or(Value::Boolean(false));
    let usages_v = args.get(2).cloned().unwrap_or(Value::Undefined);
    let usages = if let Value::Object(_) = usages_v {
        // Best-effort: read each index's string value if present.
        let mut arr: Vec<Value> = Vec::new();
        let len_v = execute::get_property(&usages_v, "length");
        if let Value::Number(n) = len_v {
            let n = n as usize;
            for i in 0..n {
                if let Ok(s) = execute::get_property_result(&usages_v, &i.to_string()) {
                    if let Value::String(_) = s {
                        arr.push(s);
                    }
                }
            }
        }
        arr
    } else {
        Vec::new()
    };
    Ok(fulfilled(host_api::object(vec![
        ("type".to_string(), Value::String("secret".into())),
        ("extractable".to_string(), extractable),
        (
            "algorithm".to_string(),
            host_api::object(vec![
                ("name".to_string(), Value::String(alg_name.into())),
                ("length".to_string(), Value::Number(length_bits as f64)),
            ]),
        ),
        (
            "usages".to_string(),
            host_api::array(
                usages
                    .into_iter()
                    .map(|v| match v {
                        Value::String(s) => Value::String(s),
                        _ => Value::Undefined,
                    })
                    .collect(),
            ),
        ),
        (
            "\0crypto:keydata".to_string(),
            crate::modules::buffer_proto::make_buffer(&buf),
        ),
    ])))
}

pub fn subtle_unsupported(
    _: &Rc<RefCell<HostState>>,
    _: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Ok(fulfilled(Value::Undefined))
}

/// Extract the raw key material from a CryptoKey object, including
/// any buffer-like value backed by the modules buffer prototype.
fn crypto_key_bytes(key: &Value) -> Result<Vec<u8>, quench_runtime::execute::VmError> {
    // First, look for a private property set by `subtle.import_key`.
    let raw = execute::get_property(key, "\0crypto:keydata");
    if !matches!(raw, Value::Undefined) {
        if let Ok(bytes) = argbytes(&raw) {
            return Ok(bytes);
        }
    }
    argbytes(key)
}

fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, length: usize) -> Vec<u8> {
    let hlen = 32usize;
    let blocks = (length + hlen - 1) / hlen;
    let mut out = Vec::with_capacity(blocks * hlen);
    for i in 1..=blocks {
        let mut t = vec![0u8; hlen];
        let mut u = {
            let mut msg = Vec::with_capacity(salt.len() + 4);
            msg.extend_from_slice(salt);
            msg.push(((i >> 24) & 0xff) as u8);
            msg.push(((i >> 16) & 0xff) as u8);
            msg.push(((i >> 8) & 0xff) as u8);
            msg.push((i & 0xff) as u8);
            hmac_sha256(password, &msg)
        };
        for x in 0..hlen {
            t[x] ^= u[x];
        }
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for x in 0..hlen {
                t[x] ^= u[x];
            }
        }
        out.extend_from_slice(&t);
    }
    out.truncate(length);
    out
}

fn read_u32_arg(
    args: &[Value],
    idx: usize,
    field: &str,
) -> Result<u32, quench_runtime::execute::VmError> {
    let v = args.get(idx).cloned().unwrap_or(Value::Undefined);
    match v {
        Value::Number(n) => {
            if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > (u32::MAX as f64) {
                Err(execute::type_error(
                    format!("`{field}` must be a non-negative integer").as_str(),
                ))
            } else {
                Ok(n as u32)
            }
        }
        _ => Err(execute::type_error(
            format!("`{field}` must be a number").as_str(),
        )),
    }
}

fn pbkdf2_params(
    args: &[Value],
) -> Result<(Vec<u8>, u32, String, Vec<u8>), quench_runtime::execute::VmError> {
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let name = execute::get_property(&algorithm, "name");
    let name_str = match name {
        Value::String(s) => s.to_string(),
        _ => return Err(execute::type_error("PBKDF2: algorithm.name required")),
    };
    if name_str != "PBKDF2" {
        return Err(execute::type_error(
            format!("PBKDF2: unsupported algorithm {name_str}").as_str(),
        ));
    }
    let salt = argbytes(&execute::get_property(&algorithm, "salt"))?;
    let iterations = match execute::get_property(&algorithm, "iterations") {
        Value::Number(n)
            if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= (u32::MAX as f64) =>
        {
            n as u32
        }
        _ => return Err(execute::type_error("PBKDF2: iterations required")),
    };
    let hash = match execute::get_property(&algorithm, "hash") {
        Value::String(s) => s.to_string(),
        _ => "SHA-256".to_string(),
    };
    if hash.as_str() != "SHA-256" {
        return Err(execute::type_error(
            format!("PBKDF2: unsupported hash {hash}").as_str(),
        ));
    }
    let base_key = args.get(1).cloned().unwrap_or(Value::Undefined);
    let password = crypto_key_bytes(&base_key)?;
    Ok((salt, iterations, hash, password))
}

pub fn subtle_derive_bits(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let (salt, iterations, _hash, password) = pbkdf2_params(args)?;
    let length = read_u32_arg(args, 2, "length")? as usize;
    if length == 0 || length > 64 * 1024 {
        return Err(execute::type_error("deriveBits: length must be 1..=65536"));
    }
    let derived = pbkdf2_hmac_sha256(&password, &salt, iterations.max(1), length);
    Ok(fulfilled(crate::modules::buffer_proto::make_buffer(
        &derived,
    )))
}

pub fn subtle_derive_key(
    _: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let (salt, iterations, _hash, password) = pbkdf2_params(args)?;
    let derived_alg = args.get(2).cloned().unwrap_or(Value::Undefined);
    let derived_name = execute::get_property(&derived_alg, "name");
    let derived_name_str = match derived_name {
        Value::String(s) => s.to_string(),
        _ => {
            return Err(execute::type_error(
                "deriveKey: derivedAlgorithm.name required",
            ))
        }
    };
    let length = match execute::get_property(&derived_alg, "length") {
        Value::Number(n) if n.is_finite() && n > 0.0 && n.fract() == 0.0 => n as usize,
        _ => {
            return Err(execute::type_error(
                "deriveKey: derivedAlgorithm.length required",
            ))
        }
    };
    if length % 8 != 0 || length == 0 {
        return Err(execute::type_error(
            "deriveKey: derivedAlgorithm.length must be a positive multiple of 8",
        ));
    }
    let byte_length = length / 8;
    if byte_length > 64 * 1024 {
        return Err(execute::type_error(
            "deriveKey: derivedAlgorithm.length too large",
        ));
    }
    let extractable = args.get(3).cloned().unwrap_or(Value::Boolean(false));
    let derived = pbkdf2_hmac_sha256(&password, &salt, iterations.max(1), byte_length);
    Ok(fulfilled(host_api::object(vec![
        ("type".to_string(), Value::String("secret".into())),
        ("extractable".to_string(), extractable),
        (
            "algorithm".to_string(),
            host_api::object(vec![
                ("name".to_string(), Value::String(derived_name_str.into())),
                ("length".to_string(), Value::Number(length as f64)),
            ]),
        ),
        ("usages".to_string(), host_api::array(vec![])),
        (
            "\0crypto:keydata".to_string(),
            crate::modules::buffer_proto::make_buffer(&derived),
        ),
    ])))
}
mod crypto_hash;
pub use crypto_hash::{
    hmac_sha1, hmac_sha256, hmac_sha384, hmac_sha512, md5_digest, sha1_digest, sha224_digest,
    sha256_digest, sha384_digest, sha512_digest,
};
