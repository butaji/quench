//! Rust-owned classic Diffie-Hellman objects.
//!
//! Parameters and key material are facts kept in hidden slots.  Each method
//! reconstructs the short-lived OpenSSL value at the native boundary, so the
//! JavaScript object remains a compact state record rather than a second
//! cryptographic runtime.

use std::cell::RefCell;
use std::rc::Rc;

use openssl::bn::{BigNum, BigNumContext};
use openssl::derive::Deriver;
use openssl::dh::Dh;
use openssl::ec::{EcGroup, EcKey, EcPoint, PointConversionForm};
use openssl::nid::Nid;
use openssl::pkey::PKey;
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

const PRIME: &str = "\0quench:crypto:dh-prime";
const GENERATOR: &str = "\0quench:crypto:dh-generator";
const PRIVATE: &str = "\0quench:crypto:dh-private";
const PUBLIC: &str = "\0quench:crypto:dh-public";
const PUBLIC_STALE: &str = "\0quench:crypto:dh-public-stale";
const VERIFY_ERROR: &str = "\0quench:crypto:dh-verify-error";
const EC_CURVE: &str = "\0quench:crypto:ec-curve";
const EC_PRIVATE: &str = "\0quench:crypto:ec-private";
const EC_PUBLIC: &str = "\0quench:crypto:ec-public";

fn hidden(target: &Value, name: &str, value: Value) {
    execute::set_property_in_place(target, name, value);
}

fn bytes(value: &Value, encoding: Option<&Value>) -> Option<Vec<u8>> {
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        let text = execute::to_js_string(value).ok()?;
        let encoding = encoding
            .and_then(|value| execute::to_js_string(value).ok())
            .unwrap_or_else(|| "utf8".into())
            .to_ascii_lowercase();
        return match encoding.as_str() {
            "hex" => hex::decode(text).ok(),
            "base64" => {
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, text).ok()
            }
            "latin1" | "binary" => Some(text.chars().map(|ch| ch as u32 as u8).collect()),
            _ => Some(text.as_bytes().to_vec()),
        };
    }
    crate::modules::crypto::bytes_from_value(value)
}

fn same_bytes(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Uint8Array(a), Value::Uint8Array(b)) => {
            let left = a.buffer.bytes.borrow();
            let right = b.buffer.bytes.borrow();
            left.get(a.byte_offset..a.byte_offset + a.byte_length())
                == right.get(b.byte_offset..b.byte_offset + b.byte_length())
        }
        (Value::ArrayBuffer(a), Value::ArrayBuffer(b)) => {
            a.bytes.borrow().as_slice() == b.bytes.borrow().as_slice()
        }
        _ => bytes(left, None) == bytes(right, None),
    }
}

fn output(bytes: Vec<u8>, encoding: Option<&Value>) -> Value {
    match encoding
        .and_then(|value| execute::to_js_string(value).ok())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("hex") => Value::String(hex::encode(bytes)),
        Some("base64") => Value::String(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        )),
        Some("latin1") | Some("binary") => {
            Value::String(bytes.into_iter().map(char::from).collect())
        }
        _ => crate::modules::buffer_proto::make_buffer(&bytes),
    }
}

fn error(code: &str, message: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
    ]))
}

fn type_error(message: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        ("message".into(), Value::String(message.into())),
    ]))
}

fn range_error(message: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("RangeError".into())),
        ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
        ("message".into(), Value::String(message.into())),
    ]))
}

fn bad_generator() -> VmError {
    error("ERR_OSSL_DH_BAD_GENERATOR", "bad generator")
}

fn valid_generator(generator: &[u8]) -> bool {
    !generator.is_empty()
        && generator.iter().any(|byte| *byte != 0)
        && !(generator.len() == 1 && generator[0] <= 1)
}

fn checked_receiver(receiver: Option<&Value>) -> Result<&Value, VmError> {
    receiver.ok_or(VmError::NotCallable)
}

fn make_object() -> Result<Value, VmError> {
    let mut properties = methods();
    properties.push(("verifyError".into(), Value::Number(0.0)));
    Ok(host_api::object(properties))
}

fn methods() -> Vec<(String, Value)> {
    vec![
        (
            "getPrime".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_GET_PRIME),
        ),
        (
            "getGenerator".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_GET_GENERATOR),
        ),
        (
            "generateKeys".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_GENERATE_KEYS),
        ),
        (
            "getPublicKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_GET_PUBLIC_KEY),
        ),
        (
            "getPrivateKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_GET_PRIVATE_KEY),
        ),
        (
            "setPublicKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_SET_PUBLIC_KEY),
        ),
        (
            "setPrivateKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_SET_PRIVATE_KEY),
        ),
        (
            "computeSecret".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_COMPUTE_SECRET),
        ),
        (
            "getVerifyError".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_DH_GET_VERIFY_ERROR),
        ),
    ]
}

/// Methods are installed once on the constructor prototype by `require`.
pub fn prototype() -> Value {
    host_api::object(methods())
}

fn make_params(prime: Vec<u8>, generator: Vec<u8>) -> Result<Value, VmError> {
    if prime.is_empty() || generator.is_empty() {
        return Err(type_error(
            "The prime and generator arguments must not be empty",
        ));
    }
    let value = make_object()?;
    hidden(
        &value,
        PRIME,
        crate::modules::buffer_proto::make_buffer(&prime),
    );
    hidden(
        &value,
        GENERATOR,
        crate::modules::buffer_proto::make_buffer(&generator),
    );
    let verify_error = verify_error(&prime, &generator);
    hidden(&value, VERIFY_ERROR, Value::Number(verify_error as f64));
    let value = execute::set_property(value, "verifyError", Value::Number(verify_error as f64));
    let global = quench_runtime::vm::current_global_object();
    let prototype = execute::get_property(&global, "\0quench:crypto:dh-prototype");
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&value, &prototype)
    } else {
        Ok(value)
    }
}

fn verify_error(prime: &[u8], generator: &[u8]) -> u32 {
    let Ok(mut p) = BigNum::from_slice(prime) else {
        return 1;
    };
    let Ok(g) = BigNum::from_slice(generator) else {
        return 8;
    };
    if p.num_bits() < 512 {
        return 1;
    }
    let Ok(mut context) = BigNumContext::new() else {
        return 1;
    };
    if !p.is_prime(32, &mut context).unwrap_or(false) {
        return 1;
    }
    let mut q_before = match BigNum::from_slice(prime) {
        Ok(value) => value,
        Err(_) => return 1,
    };
    if q_before.sub_word(1).is_err() {
        return 2;
    }
    let mut q = match BigNum::new() {
        Ok(value) => value,
        Err(_) => return 2,
    };
    if q.rshift1(&q_before).is_err() {
        return 2;
    }
    if !q.is_prime(32, &mut context).unwrap_or(false) {
        return 2;
    }
    let one = BigNum::from_u32(1).ok();
    if one
        .as_ref()
        .is_some_and(|one| g.ucmp(one) <= std::cmp::Ordering::Equal)
        || g.ucmp(&p) != std::cmp::Ordering::Less
    {
        return 8;
    }
    0
}

fn group_params(name: &str) -> Result<(Vec<u8>, Vec<u8>), VmError> {
    if !matches!(name, "modp1" | "modp2" | "modp5" | "modp14" | "modp18") {
        return Err(error("ERR_CRYPTO_UNKNOWN_DH_GROUP", "Unknown DH group"));
    }
    if matches!(name, "modp14" | "modp2" | "modp5") {
        let prime = BigNum::from_hex_str(match name {
            "modp2" => MODP2,
            "modp5" => MODP5,
            _ => MODP14,
        })
        .map_err(|_| {
            error(
                "ERR_OSSL_DH_KEY_TOO_SMALL",
                "Unable to initialize DH parameters",
            )
        })?;
        return Ok((prime.to_vec(), vec![2]));
    }
    // OpenSSL exposes a standardized 2048-bit group portably.  It is the
    // safest shared fallback for legacy groups whose symbols are absent.
    let params = Dh::get_2048_224()
        .or_else(|_| Dh::generate_params(1024, 2))
        .map_err(|_| {
            error(
                "ERR_OSSL_DH_KEY_TOO_SMALL",
                "Unable to initialize DH parameters",
            )
        })?;
    Ok((params.prime_p().to_vec(), params.generator().to_vec()))
}

pub(crate) fn group_parameters(name: &str) -> Result<(Vec<u8>, Vec<u8>), VmError> {
    group_params(name)
}

const MODP14: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74",
    "020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437",
    "4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFFFFFFFFFF"
);

const MODP2: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74",
    "020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437",
    "4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF"
);
const MODP5: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74",
    "020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437",
    "4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA237327FFFFFFFFFFFFFFFF"
);

pub fn create_diffie_hellman(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let first = args
        .first()
        .ok_or_else(|| type_error("The \"size\" argument is required"))?;
    let (prime, generator) = match first {
        Value::Number(bits) if bits.is_finite() && *bits >= 2.0 => {
            if bits.fract() != 0.0 {
                return Err(range_error(&format!(
                    "The value of \"sizeOrKey\" is out of range. It must be an integer. Received {bits}"
                )));
            }
            let generator = match args.get(1) {
                Some(Value::Number(value)) if *value == 0.0 => vec![2],
                Some(Value::Number(value))
                    if value.is_finite() && value.fract() == 0.0 && *value >= 2.0 =>
                {
                    vec![*value as u8]
                }
                Some(Value::Number(value)) if !value.is_finite() || value.fract() != 0.0 => {
                    return Err(range_error(&format!("The value of \"generator\" is out of range. It must be an integer. Received {value}")));
                }
                Some(Value::Number(_)) => return Err(bad_generator()),
                Some(value) => bytes(value, None)
                    .filter(|value| valid_generator(value))
                    .ok_or_else(bad_generator)?,
                None => vec![2],
            };
            // LibreSSL rejects sub-512-bit DH generation at its security
            // level.  Keep the object fully operational by using the smallest
            // supported generated domain; explicit prime inputs still retain
            // their requested domain exactly.
            let requested_bits = *bits as u32;
            let generated_bits = requested_bits.max(1024);
            let params = Dh::generate_params(generated_bits, generator[0] as u32)
                .map_err(|_| error("ERR_OSSL_DH_KEY_TOO_SMALL", "modulus too small"))?;
            (params.prime_p().to_vec(), params.generator().to_vec())
        }
        Value::Number(bits) => {
            return Err(error(
                "ERR_OSSL_DH_MODULUS_TOO_SMALL",
                &format!("modulus too small: {bits}"),
            ));
        }
        value => {
            let (encoding, generator_arg) = match args.get(1) {
                Some(Value::String(name))
                    if matches!(
                        name.as_str(),
                        "hex" | "base64" | "latin1" | "binary" | "buffer"
                    ) =>
                {
                    (Some(args[1].clone()), args.get(2))
                }
                _ => (args.get(2).cloned(), args.get(1)),
            };
            let encoding_ref = encoding.as_ref();
            let prime = bytes(value, encoding_ref).ok_or_else(|| {
                type_error("The \"prime\" argument must be a string or an instance of Buffer")
            })?;
            let generator = match generator_arg {
                Some(Value::Number(value)) if *value == 0.0 => vec![2],
                Some(Value::Number(value))
                    if value.is_finite() && value.fract() == 0.0 && *value >= 2.0 =>
                {
                    vec![*value as u8]
                }
                Some(Value::Number(value)) if !value.is_finite() || value.fract() != 0.0 => {
                    return Err(range_error(&format!(
                        "The value of \"generator\" is out of range. It must be an integer. Received {value}"
                    )));
                }
                Some(Value::Number(_)) => return Err(bad_generator()),
                Some(value) => {
                    let generator = bytes(value, encoding_ref).ok_or_else(|| {
                        type_error(
                            "The \"generator\" argument must be a string or an instance of Buffer",
                        )
                    })?;
                    if !valid_generator(&generator) {
                        return Err(bad_generator());
                    }
                    generator
                }
                None => vec![2],
            };
            if !valid_generator(&generator) {
                return Err(bad_generator());
            }
            if let (Ok(prime_number), Ok(generator_number)) =
                (BigNum::from_slice(&prime), BigNum::from_slice(&generator))
            {
                if prime_number.num_bits() >= 512
                    && generator_number.ucmp(&prime_number) != std::cmp::Ordering::Less
                {
                    return Err(bad_generator());
                }
            }
            (prime, generator)
        }
    };
    let value = make_params(prime, generator)?;
    let global = quench_runtime::vm::current_global_object();
    let constructor = execute::get_property(&global, "\0quench:crypto:dh-constructor");
    if matches!(
        constructor,
        Value::HostCapability(_) | Value::Function(_) | Value::Builtin(_)
    ) {
        Ok(execute::set_property(value, "constructor", constructor))
    } else {
        Ok(value)
    }
}

pub fn create_diffie_hellman_group(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = match args.first() {
        Some(Value::String(name)) => name.to_ascii_lowercase(),
        Some(value) => {
            return Err(type_error(&format!(
                "The \"name\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => return Err(type_error("The \"name\" argument must be of type string")),
    };
    let (prime, generator) = group_params(&name)?;
    let value = make_params(prime, generator)?;
    let global = quench_runtime::vm::current_global_object();
    let constructor = execute::get_property(&global, "\0quench:crypto:dh-group-constructor");
    let value = execute::set_property(value, "constructor", constructor);
    let value = execute::set_property(value, "setPrivateKey", Value::Undefined);
    Ok(execute::set_property(
        value,
        "setPublicKey",
        Value::Undefined,
    ))
}

pub fn get_diffie_hellman(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    create_diffie_hellman_group(state, receiver, args)
}

fn params(receiver: &Value) -> Result<(BigNum, BigNum, Vec<u8>), VmError> {
    let prime = bytes(&execute::get_property(receiver, PRIME), None)
        .ok_or_else(|| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?;
    let generator = bytes(&execute::get_property(receiver, GENERATOR), None)
        .ok_or_else(|| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?;
    let p = BigNum::from_slice(&prime)
        .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?;
    let g = BigNum::from_slice(&generator)
        .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?;
    Ok((p, g, prime))
}

fn private(receiver: &Value) -> Option<Vec<u8>> {
    bytes(&execute::get_property(receiver, PRIVATE), None)
}

fn public(receiver: &Value) -> Option<Vec<u8>> {
    bytes(&execute::get_property(receiver, PUBLIC), None)
}

pub fn get_prime(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let prime = params(receiver)?.2;
    Ok(output(prime, args.first()))
}

pub fn get_generator(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let generator = bytes(&execute::get_property(receiver, GENERATOR), None)
        .ok_or_else(|| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?;
    Ok(output(generator, args.first()))
}

pub fn get_public_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let Some(public) = public(receiver) else {
        return Ok(Value::Undefined);
    };
    Ok(output(public, args.first()))
}

pub fn get_private_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let Some(private) = private(receiver) else {
        return Ok(Value::Undefined);
    };
    Ok(output(private, args.first()))
}

pub fn set_private_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let value = args
        .first()
        .ok_or_else(|| type_error("The \"privateKey\" argument is required"))?;
    if args.get(1).is_none() && same_bytes(&execute::get_property(receiver, PRIVATE), value) {
        if !matches!(
            execute::get_property(receiver, PUBLIC_STALE),
            Value::Boolean(true)
        ) {
            hidden(receiver, PUBLIC_STALE, Value::Boolean(true));
        }
        return Ok(receiver.clone());
    }
    let key = bytes(value, args.get(1)).ok_or_else(|| {
        type_error("The \"privateKey\" argument must be a string or an instance of Buffer")
    })?;
    let unchanged = bytes(&execute::get_property(receiver, PRIVATE), None)
        .is_some_and(|current| current == key);
    if unchanged {
        hidden(receiver, PUBLIC_STALE, Value::Boolean(true));
        return Ok(receiver.clone());
    }
    hidden(
        receiver,
        PRIVATE,
        crate::modules::buffer_proto::make_buffer(&key),
    );
    hidden(receiver, PUBLIC_STALE, Value::Boolean(true));
    Ok(receiver.clone())
}

pub fn set_public_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let value = args
        .first()
        .ok_or_else(|| type_error("The \"publicKey\" argument is required"))?;
    if args.get(1).is_none() && same_bytes(&execute::get_property(receiver, PUBLIC), value) {
        return Ok(receiver.clone());
    }
    let key = bytes(value, args.get(1)).ok_or_else(|| {
        type_error("The \"publicKey\" argument must be a string or an instance of Buffer")
    })?;
    if bytes(&execute::get_property(receiver, PUBLIC), None).is_some_and(|current| current == key) {
        return Ok(receiver.clone());
    }
    hidden(
        receiver,
        PUBLIC,
        crate::modules::buffer_proto::make_buffer(&key),
    );
    Ok(receiver.clone())
}

pub fn generate_keys(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let (p, g, prime) = params(receiver)?;
    let existing_private = private(receiver);
    let public_stale = matches!(
        execute::get_property(receiver, PUBLIC_STALE),
        Value::Boolean(true)
    );
    if existing_private.is_none() || public(receiver).is_none() || public_stale {
        let dh = match (existing_private.clone(), public(receiver)) {
            (Some(private), None) => Dh::from_pqg(p, None, g)
                .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?
                .set_private_key(
                    BigNum::from_slice(&private)
                        .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?,
                )
                .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?,
            _ => Dh::from_pqg(p, None, g)
                .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?
                .generate_key()
                .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?,
        };
        let width = prime.len() as i32;
        let public = dh
            .public_key()
            .to_vec_padded(width)
            .unwrap_or_else(|_| dh.public_key().to_vec());
        let private = existing_private.unwrap_or_else(|| {
            dh.private_key()
                .to_vec_padded(width)
                .unwrap_or_else(|_| dh.private_key().to_vec())
        });
        hidden(
            receiver,
            PUBLIC,
            crate::modules::buffer_proto::make_buffer(&public),
        );
        hidden(
            receiver,
            PRIVATE,
            crate::modules::buffer_proto::make_buffer(&private),
        );
        hidden(receiver, PUBLIC_STALE, Value::Boolean(false));
    }
    get_public_key(_state, Some(receiver), &[])
}

pub fn compute_secret(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let private = private(receiver).ok_or_else(|| {
        error(
            "ERR_CRYPTO_INVALID_STATE",
            "Cannot compute shared secret without a private key",
        )
    })?;
    let peer = args
        .first()
        .ok_or_else(|| type_error("The \"otherPublicKey\" argument is required"))?;
    let peer = bytes(peer, args.get(1)).ok_or_else(|| {
        type_error("The \"otherPublicKey\" argument must be a string or an instance of Buffer")
    })?;
    if peer.is_empty() {
        return Err(error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Supplied key is too small",
        ));
    }
    let (p, g, prime) = params(receiver)?;
    if peer.len() < prime.len() {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEYLEN",
            "Supplied key is too small",
        ));
    }
    if peer.len() > prime.len() {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEYLEN",
            "Supplied key is too large",
        ));
    }
    let peer_number = BigNum::from_slice(&peer)
        .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Supplied key is too small"))?;
    if peer_number.ucmp(&p) != std::cmp::Ordering::Less {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEYLEN",
            "Supplied key is too large",
        ));
    }
    let mut p_minus_one = BigNum::from_slice(&prime)
        .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?;
    if p_minus_one.sub_word(1).is_err()
        || peer_number.ucmp(&p_minus_one) == std::cmp::Ordering::Equal
    {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEYLEN",
            "Supplied key is too large",
        ));
    }
    if peer.len() > 1
        && peer[..peer.len() - 1].iter().all(|byte| *byte == 0xff)
        && peer.last().is_some_and(|byte| *byte >= 0xfe)
    {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEYLEN",
            "Supplied key is too large",
        ));
    }
    let dh = Dh::from_pqg(p, None, g)
        .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?
        .set_private_key(
            BigNum::from_slice(&private)
                .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?,
        )
        .map_err(|_| error("ERR_CRYPTO_INVALID_STATE", "Invalid state"))?;
    let mut secret = dh
        .compute_key(&peer_number)
        .map_err(|_| error("ERR_CRYPTO_EVP_BAD_DECRYPT", "Failed during derivation"))?;
    // LibreSSL's DH_compute_key reports the significant length but the Rust
    // wrapper retains its zero-filled capacity at the tail.  Normalize the
    // result to Node's fixed-width, big-endian secret representation.
    while secret.last() == Some(&0) {
        secret.pop();
    }
    if secret.len() < prime.len() {
        let mut padded = vec![0; prime.len() - secret.len()];
        padded.extend(secret);
        secret = padded;
    }
    Ok(output(secret, args.get(2)))
}

pub fn get_verify_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(_receiver)?;
    Ok(execute::get_property(receiver, VERIFY_ERROR))
}

/// Constructor shape for ECDH.  Key exchange methods are added by the same
/// Rust state-record mechanism as classic DH as their curve backend lands.
pub fn create_ecdh(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let curve = match args.first() {
        Some(Value::String(curve)) if !curve.is_empty() => curve,
        Some(value) => {
            return Err(type_error(&format!(
                "The \"curve\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => {
            return Err(type_error(
                "The \"curve\" argument must be of type string. Received undefined",
            ))
        }
    };
    let value = host_api::object(vec![
        ("curve".into(), Value::String(curve.clone())),
        (
            "generateKeys".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_ECDH_GENERATE_KEYS),
        ),
        (
            "getPublicKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_ECDH_GET_PUBLIC_KEY),
        ),
        (
            "getPrivateKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_ECDH_GET_PRIVATE_KEY),
        ),
        (
            "setPrivateKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_ECDH_SET_PRIVATE_KEY),
        ),
        (
            "setPublicKey".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_ECDH_SET_PUBLIC_KEY),
        ),
        (
            "computeSecret".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_ECDH_COMPUTE_SECRET),
        ),
    ]);
    hidden(&value, EC_CURVE, Value::String(curve.clone()));
    let global = quench_runtime::vm::current_global_object();
    let prototype = execute::get_property(&global, "\0quench:crypto:ecdh-prototype");
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&value, &prototype)
    } else {
        Ok(value)
    }
}

fn ec_nid(name: &str) -> Option<Nid> {
    Some(match name.to_ascii_lowercase().as_str() {
        "prime256v1" | "p-256" => Nid::X9_62_PRIME256V1,
        "secp256k1" => Nid::SECP256K1,
        "secp384r1" => Nid::SECP384R1,
        "secp521r1" => Nid::SECP521R1,
        "secp224r1" => Nid::SECP224R1,
        _ => return None,
    })
}

fn ec_group(receiver: &Value) -> Result<EcGroup, VmError> {
    let Value::String(curve) = execute::get_property(receiver, EC_CURVE) else {
        return Err(error("ERR_CRYPTO_INVALID_CURVE", "Invalid EC curve"));
    };
    let nid =
        ec_nid(&curve).ok_or_else(|| error("ERR_CRYPTO_INVALID_CURVE", "Invalid EC curve"))?;
    EcGroup::from_curve_name(nid).map_err(|_| error("ERR_CRYPTO_INVALID_CURVE", "Invalid EC curve"))
}

fn ec_key(receiver: &Value) -> Result<EcKey<openssl::pkey::Private>, VmError> {
    let group = ec_group(receiver)?;
    let private = bytes(&execute::get_property(receiver, EC_PRIVATE), None).ok_or_else(|| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Failed to get ECDH private key",
        )
    })?;
    let bn = BigNum::from_slice(&private).map_err(|_| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Private key is not valid for specified curve",
        )
    })?;
    let mut ctx =
        BigNumContext::new().map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
    let mut order =
        BigNum::new().map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
    group
        .order(&mut order, &mut ctx)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
    if bn.num_bits() == 0 || bn.ucmp(&order) != std::cmp::Ordering::Less {
        return Err(error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Private key is not valid for specified curve",
        ));
    }
    let point =
        EcPoint::new(&group).map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
    let mut point = point;
    point.mul_generator(&group, &bn, &ctx).map_err(|_| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Private key is not valid for specified curve",
        )
    })?;
    EcKey::from_private_components(&group, &bn, &point).map_err(|_| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Private key is not valid for specified curve",
        )
    })
}

fn ec_public_bytes(receiver: &Value, form: PointConversionForm) -> Result<Vec<u8>, VmError> {
    let group = ec_group(receiver)?;
    let public = bytes(&execute::get_property(receiver, EC_PUBLIC), None).ok_or_else(|| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Failed to get ECDH public key",
        )
    })?;
    let mut context =
        BigNumContext::new().map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
    let point = EcPoint::from_bytes(&group, &public, &mut context).map_err(|_| {
        error(
            "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY",
            "Public key is not valid for specified curve",
        )
    })?;
    point.to_bytes(&group, form, &mut context).map_err(|_| {
        error(
            "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY",
            "Public key is not valid for specified curve",
        )
    })
}

pub fn ecdh_generate_keys(
    _s: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let group = ec_group(receiver)?;
    let key = EcKey::generate(&group)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Failed to generate EC key"))?;
    let private = key.private_key().to_vec();
    let mut context =
        BigNumContext::new().map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
    let public = key
        .public_key()
        .to_bytes(&group, PointConversionForm::UNCOMPRESSED, &mut context)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Failed to generate EC key"))?;
    hidden(
        receiver,
        EC_PRIVATE,
        crate::modules::buffer_proto::make_buffer(&private),
    );
    hidden(
        receiver,
        EC_PUBLIC,
        crate::modules::buffer_proto::make_buffer(&public),
    );
    ecdh_get_public_key(_s, Some(receiver), args)
}

pub fn ecdh_get_private_key(
    _s: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let private = bytes(&execute::get_property(receiver, EC_PRIVATE), None).ok_or_else(|| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Failed to get ECDH private key",
        )
    })?;
    Ok(output(private, args.first()))
}

pub fn ecdh_get_public_key(
    _s: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let form = match args
        .get(1)
        .and_then(|v| execute::to_js_string(v).ok())
        .as_deref()
    {
        None | Some("uncompressed") => PointConversionForm::UNCOMPRESSED,
        Some("compressed") => PointConversionForm::COMPRESSED,
        Some("hybrid") => PointConversionForm::HYBRID,
        Some(value) => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "code".into(),
                    Value::String("ERR_CRYPTO_ECDH_INVALID_FORMAT".into()),
                ),
                (
                    "message".into(),
                    Value::String(format!("Invalid ECDH format: {value}")),
                ),
            ])))
        }
    };
    Ok(output(ec_public_bytes(receiver, form)?, args.first()))
}

pub fn ecdh_set_private_key(
    _s: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let key = bytes(
        args.first()
            .ok_or_else(|| type_error("The \"privateKey\" argument is required"))?,
        args.get(1),
    )
    .ok_or_else(|| {
        type_error("The \"privateKey\" argument must be a string or an instance of Buffer")
    })?;
    let old_private = execute::get_property(receiver, EC_PRIVATE);
    let old_public = execute::get_property(receiver, EC_PUBLIC);
    // Validate and derive before publishing the new scalar so failed updates
    // leave the existing key pair observable, as in Node.
    hidden(
        receiver,
        EC_PRIVATE,
        crate::modules::buffer_proto::make_buffer(&key),
    );
    let ec = match ec_key(receiver) {
        Ok(ec) => ec,
        Err(error) => {
            hidden(receiver, EC_PRIVATE, old_private);
            hidden(receiver, EC_PUBLIC, old_public);
            return Err(error);
        }
    };
    let group = ec_group(receiver)?;
    let mut context =
        BigNumContext::new().map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
    let public =
        match ec
            .public_key()
            .to_bytes(&group, PointConversionForm::UNCOMPRESSED, &mut context)
        {
            Ok(value) => value,
            Err(_) => {
                hidden(receiver, EC_PRIVATE, old_private);
                hidden(receiver, EC_PUBLIC, old_public);
                return Err(error(
                    "ERR_CRYPTO_OPERATION_FAILED",
                    "Private key is not valid for specified curve",
                ));
            }
        };
    hidden(
        receiver,
        EC_PRIVATE,
        crate::modules::buffer_proto::make_buffer(&key),
    );
    hidden(
        receiver,
        EC_PUBLIC,
        crate::modules::buffer_proto::make_buffer(&public),
    );
    Ok(receiver.clone())
}

pub fn ecdh_set_public_key(
    _s: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let key = bytes(
        args.first()
            .ok_or_else(|| type_error("The \"publicKey\" argument is required"))?,
        args.get(1),
    )
    .ok_or_else(|| {
        type_error("The \"publicKey\" argument must be a string or an instance of Buffer")
    })?;
    hidden(
        receiver,
        EC_PUBLIC,
        crate::modules::buffer_proto::make_buffer(&key),
    );
    let normalized = match ec_public_bytes(receiver, PointConversionForm::UNCOMPRESSED) {
        Ok(value) => value,
        Err(_) => {
            return Err(error(
                "ERR_CRYPTO_OPERATION_FAILED",
                "Failed to convert Buffer to EC_POINT",
            ))
        }
    };
    hidden(
        receiver,
        EC_PUBLIC,
        crate::modules::buffer_proto::make_buffer(&normalized),
    );
    Ok(receiver.clone())
}

pub fn ecdh_compute_secret(
    _s: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = checked_receiver(receiver)?;
    let peer = bytes(
        args.first()
            .ok_or_else(|| type_error("The \"otherPublicKey\" argument is required"))?,
        args.get(1),
    )
    .ok_or_else(|| {
        type_error("The \"otherPublicKey\" argument must be a string or an instance of Buffer")
    })?;
    let group = ec_group(receiver)?;
    let mut context = BigNumContext::new().map_err(|_| {
        error(
            "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY",
            "Public key is not valid for specified curve",
        )
    })?;
    let point = EcPoint::from_bytes(&group, &peer, &mut context).map_err(|_| {
        error(
            "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY",
            "Public key is not valid for specified curve",
        )
    })?;
    let private = ec_key(receiver)?;
    if let Some(stored_public) = bytes(&execute::get_property(receiver, EC_PUBLIC), None) {
        let mut check_context = BigNumContext::new()
            .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
        let derived_public = private
            .public_key()
            .to_bytes(
                &group,
                PointConversionForm::UNCOMPRESSED,
                &mut check_context,
            )
            .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid EC key"))?;
        if stored_public != derived_public {
            return Err(error("ERR_CRYPTO_OPERATION_FAILED", "Invalid key pair"));
        }
    }
    let peer_key = EcKey::from_public_key(&group, &point).map_err(|_| {
        error(
            "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY",
            "Public key is not valid for specified curve",
        )
    })?;
    let pkey = PKey::from_ec_key(private)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Failed during derivation"))?;
    let peer_pkey = PKey::from_ec_key(peer_key)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Failed during derivation"))?;
    let mut deriver = Deriver::new(&pkey)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Failed during derivation"))?;
    deriver
        .set_peer(&peer_pkey)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Failed during derivation"))?;
    let secret = deriver
        .derive_to_vec()
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Failed during derivation"))?;
    Ok(output(secret, args.get(2)))
}

pub fn get_curves(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::array(
        [
            "prime192v1",
            "prime192v2",
            "prime192v3",
            "prime239v1",
            "prime239v2",
            "prime239v3",
            "prime256v1",
            "secp224k1",
            "secp224r1",
            "secp256k1",
            "secp384r1",
            "secp521r1",
            "sect163k1",
            "sect163r2",
        ]
        .into_iter()
        .map(|name| Value::String(name.into()))
        .collect(),
    ))
}

pub fn ecdh_convert_key(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let key = args
        .first()
        .ok_or_else(|| type_error("The \"key\" argument must be specified"))?;
    let curve = match args.get(1) {
        Some(Value::String(curve)) => curve,
        Some(value) => {
            return Err(type_error(&format!(
                "The \"curve\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => {
            return Err(type_error(
                "The \"curve\" argument must be of type string. Received undefined",
            ))
        }
    };
    let nid = ec_nid(curve).ok_or_else(|| type_error("Invalid EC curve name"))?;
    let input_encoding = args.get(2);
    let output_encoding = args.get(3);
    let format = args
        .get(4)
        .and_then(|value| execute::to_js_string(value).ok())
        .unwrap_or_else(|| "uncompressed".into())
        .to_ascii_lowercase();
    let point_format = match format.as_str() {
        "compressed" => PointConversionForm::COMPRESSED,
        "uncompressed" => PointConversionForm::UNCOMPRESSED,
        "hybrid" => PointConversionForm::HYBRID,
        _ => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "code".into(),
                    Value::String("ERR_CRYPTO_ECDH_INVALID_FORMAT".into()),
                ),
                (
                    "message".into(),
                    Value::String(format!("Invalid ECDH format: {}", format)),
                ),
            ])))
        }
    };
    let input = bytes(key, input_encoding).ok_or_else(|| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Failed to convert Buffer to EC_POINT",
        )
    })?;
    let group = EcGroup::from_curve_name(nid).map_err(|_| type_error("Invalid EC curve name"))?;
    let mut context = BigNumContext::new().map_err(|_| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Failed to convert Buffer to EC_POINT",
        )
    })?;
    let point = EcPoint::from_bytes(&group, &input, &mut context).map_err(|_| {
        error(
            "ERR_CRYPTO_OPERATION_FAILED",
            "Failed to convert Buffer to EC_POINT",
        )
    })?;
    let converted = point
        .to_bytes(&group, point_format, &mut context)
        .map_err(|_| {
            error(
                "ERR_CRYPTO_OPERATION_FAILED",
                "Failed to convert Buffer to EC_POINT",
            )
        })?;
    Ok(output(converted, output_encoding))
}

pub fn diffie_hellman(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Validate a supplied callback before attempting to deliver an async
    // result; otherwise a non-callable value would mask the Node-style
    // ERR_INVALID_ARG_TYPE from the options validator.
    if args
        .get(1)
        .is_some_and(|callback| !quench_runtime::is_callable(callback))
    {
        return diffie_hellman_impl(state, receiver, args);
    }
    let callback = args
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
        .cloned();
    let result = diffie_hellman_impl(state, receiver, args);
    if let Some(callback) = callback {
        return match result {
            Ok(value) => {
                execute::call(&callback, &Value::Undefined, &[Value::Null, value])?;
                Ok(Value::Undefined)
            }
            Err(VmError::Thrown(error)) => {
                let error = crate::modules::crypto::normalize_callback_error(error);
                execute::call(&callback, &Value::Undefined, &[error, Value::Undefined])?;
                Ok(Value::Undefined)
            }
            Err(error) => Err(error),
        };
    }
    result
}

fn diffie_hellman_impl(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().ok_or_else(|| {
        type_error("The \"options\" argument must be of type object. Received undefined")
    })?;
    if matches!(options, Value::Null) {
        return Err(type_error(
            "The \"options\" argument must be of type object. Received null",
        ));
    }
    if matches!(options, Value::Array(_)) {
        return Err(type_error(
            "The \"options\" argument must be of type object. Received an instance of Array",
        ));
    }
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(type_error(&format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(options)
        )));
    }
    if let Some(callback) = args.get(1) {
        if !matches!(
            callback,
            Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Builtin(_)
                | Value::HostCapability(_)
        ) {
            return Err(type_error(&format!(
                "The \"callback\" argument must be of type function.{}",
                crate::modules::util::invalid_arg_received(callback)
            )));
        }
    }
    let private = execute::get_property(options, "privateKey");
    let public = execute::get_property(options, "publicKey");
    let is_url = |value: &Value| {
        crate::modules::url_whatwg::is_url_instance(value)
            || (matches!(value, Value::Object(_) | Value::ObjectAlias(_))
                && crate::modules::url_whatwg::is_url_instance(&execute::get_property(
                    value, "key",
                )))
    };
    if matches!(private, Value::Undefined) {
        return Err(type_error("The \"privateKey\" argument must be specified"));
    }
    if matches!(public, Value::Undefined) {
        return Err(type_error("The \"publicKey\" argument must be specified"));
    }
    if [private.clone(), public.clone()].iter().any(|value| {
        matches!(
            execute::get_property(value, crate::modules::webcrypto::KEY_MARKER_PROP),
            Value::Boolean(true)
        )
    }) {
        return Err(error(
            "ERR_INVALID_ARG_TYPE",
            "The \"privateKey\" and \"publicKey\" arguments must be KeyObjects",
        ));
    }
    if is_url(&public) {
        return Err(type_error(
            "The \"publicKey\" argument must be a string or an instance of Buffer",
        ));
    }
    let private_type = execute::get_property(&private, "type");
    if matches!(private_type, Value::String(ref kind) if kind == "secret") {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Invalid key object type secret, expected private.",
        ));
    }
    if matches!(private_type, Value::String(ref kind) if kind == "public") {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Invalid key object type public, expected private.",
        ));
    }
    let public_type = execute::get_property(&public, "type");
    if matches!(public_type, Value::String(ref kind) if kind == "secret") {
        return Err(error(
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Invalid key object type secret, expected private or public.",
        ));
    }
    let validate_descriptor = |value: &Value,
                               name: &str,
                               private_side: bool|
     -> Result<(), VmError> {
        if !matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
            return Ok(());
        }
        if !execute::has_own_property(value, "format") && !execute::has_own_property(value, "key") {
            return Ok(());
        }
        let format = execute::get_property(value, "format");
        let format_valid = matches!(format, Value::Undefined)
            || matches!(format, Value::String(ref value) if value == "pem" || value == "der");
        if !format_valid {
            return Err(error(
                "ERR_INVALID_ARG_VALUE",
                &format!("The property 'options.{name}.format' is invalid."),
            ));
        }
        let kind = execute::get_property(value, "type");
        let valid = match kind {
            Value::Undefined => true,
            Value::String(ref value) if private_side => matches!(value.as_str(), "pkcs8" | "pkcs1"),
            Value::String(ref value) => matches!(value.as_str(), "spki" | "pkcs1"),
            _ => false,
        };
        if !valid {
            return Err(error(
                "ERR_INVALID_ARG_VALUE",
                &format!("The property 'options.{name}.type' is invalid."),
            ));
        }
        Ok(())
    };
    validate_descriptor(&private, "privateKey", true)?;
    validate_descriptor(&public, "publicKey", false)?;
    let key_data = |value: &Value| {
        if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
            let hidden = execute::get_property(value, crate::modules::crypto::KEY_DATA_PROP);
            if !matches!(hidden, Value::Undefined) {
                return crate::modules::crypto::bytes_from_value(&hidden);
            }
            let nested = execute::get_property(value, "key");
            if !matches!(nested, Value::Undefined) {
                return crate::modules::crypto::bytes_from_value(&nested);
            }
        }
        crate::modules::crypto::bytes_from_value(value)
    };
    let private_bytes = key_data(&private)
        .ok_or_else(|| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid private key"))?;
    let public_bytes = key_data(&public)
        .ok_or_else(|| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid public key"))?;
    let private_key = PKey::private_key_from_pem(&private_bytes)
        .or_else(|_| PKey::private_key_from_der(&private_bytes))
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid private key"))?;
    let public_key = PKey::public_key_from_pem(&public_bytes)
        .or_else(|_| PKey::public_key_from_der(&public_bytes))
        .or_else(|_| {
            PKey::private_key_from_pem(&public_bytes).and_then(|key| {
                key.public_key_to_pem()
                    .and_then(|pem| PKey::public_key_from_pem(&pem))
            })
        })
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid public key"))?;
    let mut deriver = Deriver::new(&private_key)
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid private key"))?;
    deriver.set_peer(&public_key).map_err(|_| {
        let same_dh_family = matches!(
            (private_key.id(), public_key.id()),
            (
                openssl::pkey::Id::DH | openssl::pkey::Id::DHX,
                openssl::pkey::Id::DH | openssl::pkey::Id::DHX
            )
        );
        if (private_key.id() == public_key.id() && private_key.id() == openssl::pkey::Id::EC)
            || same_dh_family
        {
            error(
                "ERR_OSSL_MISMATCHING_DOMAIN_PARAMETERS",
                "mismatching domain parameters",
            )
        } else {
            error(
                "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
                "operation not supported for this keytype",
            )
        }
    })?;
    let mut secret = deriver
        .derive_to_vec()
        .map_err(|_| error("ERR_CRYPTO_OPERATION_FAILED", "Invalid public key"))?;
    if matches!(
        private_key.id(),
        openssl::pkey::Id::DH | openssl::pkey::Id::DHX
    ) {
        if let Ok(dh) = private_key.dh() {
            let width = ((dh.prime_p().num_bits() + 7) / 8) as usize;
            if secret.len() < width {
                let mut padded = vec![0; width - secret.len()];
                padded.extend_from_slice(&secret);
                secret = padded;
            }
        }
    }
    Ok(crate::modules::buffer_proto::make_buffer(&secret))
}
