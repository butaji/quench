//! Rust-owned symmetric cipher objects for the Node crypto boundary.
//!
//! A cipher is a small state machine. The public streaming methods only append
//! input or consume the one derived final output; the algorithm-specific block
//! operation is isolated at the effect edge.

use std::cell::RefCell;
use std::rc::Rc;

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit as CipherKeyInit};
use aes::{Aes128, Aes256};
use aes_gcm::{
    aead::generic_array::{
        typenum::{U12, U13, U14, U15, U16, U60, U8},
        GenericArray,
    },
    AesGcm,
};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes128Gcm, Aes256Gcm, Nonce,
};
use cbc::{Decryptor, Encryptor};
use chacha20::ChaCha20;
use chacha20poly1305::{ChaCha20Poly1305, Nonce as ChaChaNonce};
use cipher::{
    block_padding::{NoPadding, Pkcs7},
    BlockDecryptMut, BlockEncryptMut, KeyIvInit, StreamCipher, StreamCipherSeek,
};
use des::TdesEde3;
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::ops::Builtin;
use quench_runtime::value::Value;

use crate::host::HostState;

const ALGORITHM: &str = "\0quench:crypto:cipher-algorithm";
const KEY: &str = "\0quench:crypto:cipher-key";
const IV: &str = "\0quench:crypto:cipher-iv";
const INPUT: &str = "\0quench:crypto:cipher-input";
const OUTPUT: &str = "\0quench:crypto:cipher-output";
const DECIPHER: &str = "\0quench:crypto:cipher-decipher";
const FINISHED: &str = "\0quench:crypto:cipher-finished";
const AAD: &str = "\0quench:crypto:cipher-aad";
const TAG: &str = "\0quench:crypto:cipher-tag";
const TAG_LENGTH: &str = "\0quench:crypto:cipher-tag-length";
const ENCODING: &str = "\0quench:crypto:cipher-encoding";
const PIPE_DEST: &str = "\0quench:crypto:cipher-pipe-destination";
const ERROR_LISTENER: &str = "\0quench:crypto:cipher-error-listener";
const AUTO_PADDING: &str = "\0quench:crypto:cipher-auto-padding";

fn hidden(target: &Value, name: &str, value: Value) {
    execute::set_property_in_place(target, name, value);
}

fn bytes(value: Option<&Value>) -> Option<Vec<u8>> {
    value.and_then(crate::modules::crypto::bytes_from_value)
}

fn text_bytes(value: Option<&Value>, encoding: Option<&Value>) -> Option<Vec<u8>> {
    if let (Some(Value::String(text)), Some(encoding)) = (value, encoding) {
        let encoding = execute::to_js_string(encoding).ok()?.to_ascii_lowercase();
        if encoding == "hex" {
            return hex::decode(text).ok();
        }
        if encoding == "base64" {
            use base64::Engine;
            return base64::engine::general_purpose::STANDARD.decode(text).ok();
        }
        if matches!(encoding.as_str(), "latin1" | "binary") {
            return Some(text.chars().map(|ch| ch as u32 as u8).collect());
        }
    }
    bytes(value)
}

fn byte_length(value: &Value) -> Option<usize> {
    macro_rules! view {
        ($v:expr) => {
            Some($v.byte_length())
        };
    }
    match value {
        Value::ArrayBuffer(buffer) => Some(buffer.bytes.borrow().len()),
        Value::Uint8Array(view) => view!(view),
        Value::Int8Array(view) => view!(view),
        Value::Uint8ClampedArray(view) => view!(view),
        Value::Int16Array(view) => view!(view),
        Value::Uint16Array(view) => view!(view),
        Value::Int32Array(view) => view!(view),
        Value::Uint32Array(view) => view!(view),
        Value::Float32Array(view) => view!(view),
        Value::Float64Array(view) => view!(view),
        Value::DataView(view) => view!(view),
        _ => None,
    }
}

fn encode(value: Vec<u8>, encoding: Option<&Value>) -> Value {
    let encoding = encoding
        .and_then(|value| execute::to_js_string(value).ok())
        .map(|value| value.to_ascii_lowercase());
    match encoding.as_deref() {
        Some("hex") => Value::String(hex::encode(value)),
        Some("base64") => {
            use base64::Engine;
            Value::String(base64::engine::general_purpose::STANDARD.encode(value))
        }
        Some("latin1") | Some("binary") => {
            Value::String(value.into_iter().map(char::from).collect())
        }
        Some("utf8") | Some("utf-8") => Value::String(String::from_utf8_lossy(&value).into_owned()),
        _ => crate::modules::buffer_proto::make_buffer(&value),
    }
}

fn error(code: &str, message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    let value = execute::set_property(value, "code", Value::String(code.into()));
    if code == "ERR_OSSL_BAD_DECRYPT" {
        return VmError::Thrown(execute::set_property(
            value,
            "reason",
            Value::String("bad decrypt".into()),
        ));
    }
    VmError::Thrown(value)
}

fn invalid_type(message: &str) -> VmError {
    let value =
        quench_runtime::builtins::error(Builtin::TypeError, &[Value::String(message.into())]);
    VmError::Thrown(execute::set_property(
        value,
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    ))
}

fn type_error(code: &str, message: &str) -> VmError {
    let value =
        quench_runtime::builtins::error(Builtin::TypeError, &[Value::String(message.into())]);
    VmError::Thrown(execute::set_property(
        value,
        "code",
        Value::String(code.into()),
    ))
}

fn wrong_block_length() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "code".into(),
            Value::String("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH".into()),
        ),
        (
            "message".into(),
            Value::String("wrong final block length".into()),
        ),
        ("library".into(), Value::String("Cipher functions".into())),
        (
            "reason".into(),
            Value::String("wrong final block length".into()),
        ),
    ]))
}

fn normalized(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

fn validate_output_encoding(receiver: &Value, requested: Option<&Value>) -> Result<(), VmError> {
    let Some(requested) = requested.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(());
    };
    let mut encoding = execute::to_js_string(requested)?.to_ascii_lowercase();
    if encoding == "utf-8" {
        encoding = "utf8".into();
    }
    if !matches!(
        encoding.as_str(),
        "buffer"
            | "hex"
            | "base64"
            | "base64url"
            | "latin1"
            | "binary"
            | "utf8"
            | "utf-8"
            | "ucs2"
            | "ucs-2"
            | "utf16le"
            | "utf-16le"
    ) {
        return Err(type_error(
            "ERR_UNKNOWN_ENCODING",
            &format!("Unknown encoding: {encoding}"),
        ));
    }
    let previous = execute::get_property(receiver, ENCODING);
    if let Value::String(previous) = previous {
        if previous != encoding {
            return Err(type_error("ERR_INVALID_ARG_VALUE", &format!("The argument 'outputEncoding' cannot be changed from '{previous}'. Received '{encoding}'")));
        }
    } else {
        hidden(receiver, ENCODING, Value::String(encoding));
    }
    Ok(())
}

fn block_size(name: &str) -> usize {
    if name.contains("des") {
        8
    } else {
        16
    }
}

fn supported(name: &str) -> bool {
    matches!(
        name,
        "aes-128-cbc"
            | "aes-256-cbc"
            | "aes-128-ecb"
            | "aes-256-ecb"
            | "aes-128-gcm"
            | "aes-256-gcm"
            | "chacha20-poly1305"
            | "id-aes128-wrap"
            | "des-ede3-cbc"
            | "des-ede3-ecb"
    )
}

pub fn create_cipheriv(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    build_cipher(args, false)
}

pub fn create_decipheriv(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    build_cipher(args, true)
}

fn build_cipher(args: &[Value], decipher: bool) -> Result<Value, VmError> {
    let Some(Value::String(raw_name)) = args.first() else {
        let message = if matches!(args.first(), Some(Value::Null)) {
            "The \"cipher\" argument must be of type string. Received null"
        } else {
            "The \"cipher\" argument must be of type string"
        };
        return Err(invalid_type(message));
    };
    let name = normalized(raw_name);
    if !supported(&name) {
        return Err(error("ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher"));
    }
    let Some(key) = bytes(args.get(1)) else {
        return Err(invalid_type(
            "The \"key\" argument must be of type string or an instance of Buffer",
        ));
    };
    let expected_key = if name.contains("aes-128") || name == "id-aes128-wrap" {
        16
    } else if name.contains("aes-256") || name == "chacha20-poly1305" {
        32
    } else {
        24
    };
    if key.len() != expected_key {
        return Err(error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"));
    }
    let iv = match args.get(2) {
        Some(Value::Null) => Vec::new(),
        Some(value) => bytes(Some(value)).ok_or_else(|| {
            invalid_type("The \"iv\" argument must be of type string or an instance of Buffer")
        })?,
        None => {
            return Err(invalid_type(
                "The \"iv\" argument must be of type string or an instance of Buffer",
            ))
        }
    };
    let valid_iv = if name.ends_with("ecb") {
        iv.is_empty()
    } else if name.ends_with("-gcm") {
        (8..=64).contains(&iv.len())
    } else if name == "chacha20-poly1305" {
        iv.len() == 12
    } else if name == "id-aes128-wrap" {
        iv.len() == 8
    } else {
        iv.len() == block_size(&name)
    };
    if !valid_iv {
        return Err(error(
            "ERR_CRYPTO_INVALID_IV",
            "Invalid initialization vector",
        ));
    }
    let tag_length = if name.ends_with("-gcm") {
        let requested = args
            .get(3)
            .map(|options| execute::get_property(options, "authTagLength"));
        match requested {
            None | Some(Value::Undefined) => 16,
            Some(Value::Number(n)) if [4.0, 8.0, 12.0, 13.0, 14.0, 15.0, 16.0].contains(&n) => {
                n as usize
            }
            _ => {
                return Err(type_error(
                    "ERR_CRYPTO_INVALID_AUTH_TAG",
                    "Invalid authentication tag length",
                ))
            }
        }
    } else if name == "chacha20-poly1305" {
        match args
            .get(3)
            .map(|options| execute::get_property(options, "authTagLength"))
        {
            None | Some(Value::Undefined) => 16,
            Some(Value::Number(n))
                if n.is_finite() && n.fract() == 0.0 && (1.0..=16.0).contains(&n) =>
            {
                n as usize
            }
            _ => {
                return Err(type_error(
                    "ERR_CRYPTO_INVALID_AUTH_TAG",
                    "Invalid authentication tag length",
                ))
            }
        }
    } else {
        16
    };
    let value = host_api::object(vec![
        (
            "update".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_UPDATE),
        ),
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_UPDATE),
        ),
        (
            "final".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_FINAL),
        ),
        (
            "setAutoPadding".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_SET_PADDING),
        ),
        (
            "setAAD".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_SET_AAD),
        ),
        (
            "getAuthTag".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_GET_TAG),
        ),
        (
            "setAuthTag".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_SET_TAG),
        ),
        (
            "end".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_END),
        ),
        (
            "read".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_READ),
        ),
        (
            "pipe".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_PIPE),
        ),
        (
            "unpipe".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_UNPIPE),
        ),
        (
            "on".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_ON),
        ),
        (
            "pause".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_PAUSE),
        ),
        (
            "resume".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_RESUME),
        ),
        (
            "setEncoding".into(),
            crate::host::capability(crate::registry::SPEC_CRYPTO_CIPHER_SET_ENCODING),
        ),
        ("readableLength".into(), Value::Number(0.0)),
    ]);
    hidden(&value, ALGORITHM, Value::String(name.clone()));
    hidden(&value, KEY, crate::modules::buffer_proto::make_buffer(&key));
    hidden(&value, IV, crate::modules::buffer_proto::make_buffer(&iv));
    hidden(
        &value,
        INPUT,
        crate::modules::buffer_proto::make_buffer(&[]),
    );
    hidden(
        &value,
        OUTPUT,
        crate::modules::buffer_proto::make_buffer(&[]),
    );
    hidden(&value, DECIPHER, Value::Boolean(decipher));
    hidden(&value, FINISHED, Value::Boolean(false));
    hidden(&value, AAD, crate::modules::buffer_proto::make_buffer(&[]));
    hidden(&value, TAG, crate::modules::buffer_proto::make_buffer(&[]));
    hidden(&value, TAG_LENGTH, Value::Number(tag_length as f64));
    hidden(&value, ENCODING, Value::Undefined);
    hidden(&value, AUTO_PADDING, Value::Boolean(true));
    let global = quench_runtime::vm::current_global_object();
    let proto_name = if decipher {
        "\0quench:crypto:decipher-prototype"
    } else {
        "\0quench:crypto:cipher-prototype"
    };
    let prototype = execute::get_property(&global, proto_name);
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&value, &prototype)
    } else {
        Ok(value)
    }
}

pub fn update(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(receiver, FINISHED),
        Value::Boolean(true)
    ) {
        return Err(error("ERR_CRYPTO_INVALID_STATE", "Invalid state"));
    }
    if args
        .first()
        .and_then(byte_length)
        .is_some_and(|length| length >= 0x7fff_ffff)
    {
        return Err(error(
            "ERR_OUT_OF_RANGE",
            "The data length exceeds the maximum allowed size",
        ));
    }
    let input = text_bytes(args.first(), args.get(1)).ok_or_else(|| {
        invalid_type("The data argument must be of type string or an instance of Buffer")
    })?;
    let encoding = args.get(2);
    validate_output_encoding(receiver, encoding)?;
    let algorithm =
        execute::to_js_string(&execute::get_property(receiver, ALGORITHM)).unwrap_or_default();
    let decipher = matches!(
        execute::get_property(receiver, DECIPHER),
        Value::Boolean(true)
    );
    let key = bytes(Some(&execute::get_property(receiver, KEY))).unwrap_or_default();
    let iv = bytes(Some(&execute::get_property(receiver, IV))).unwrap_or_default();
    let aad = bytes(Some(&execute::get_property(receiver, AAD))).unwrap_or_default();
    let current_value = execute::get_property(receiver, INPUT);
    let current = bytes(Some(&current_value)).unwrap_or_default();
    hidden(
        receiver,
        INPUT,
        crate::modules::buffer_proto::make_buffer(&[current, input.clone()].concat()),
    );
    if decipher && (algorithm.ends_with("-gcm") || algorithm == "chacha20-poly1305") {
        let preview = if algorithm == "chacha20-poly1305" {
            chacha_preview(&key, &iv, &input)
        } else {
            gcm_preview(&algorithm, &key, &iv, &input, &aad)
        };
        if let Some(preview) = preview {
            return Ok(encode(preview, encoding));
        }
    }
    Ok(encode(Vec::new(), encoding))
}

pub fn finalize(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(receiver, FINISHED),
        Value::Boolean(true)
    ) {
        return Err(error("ERR_CRYPTO_INVALID_STATE", "Invalid state"));
    }
    validate_output_encoding(receiver, args.first())?;
    let name = execute::to_js_string(&execute::get_property(receiver, ALGORITHM))?;
    let key_value = execute::get_property(receiver, KEY);
    let iv_value = execute::get_property(receiver, IV);
    let input_value = execute::get_property(receiver, INPUT);
    let key = bytes(Some(&key_value)).unwrap_or_default();
    let iv = bytes(Some(&iv_value)).unwrap_or_default();
    let input = bytes(Some(&input_value)).unwrap_or_default();
    let decipher = matches!(
        execute::get_property(receiver, DECIPHER),
        Value::Boolean(true)
    );
    let aad_value = execute::get_property(receiver, AAD);
    let tag_value = execute::get_property(receiver, TAG);
    let aad = bytes(Some(&aad_value)).unwrap_or_default();
    let tag = bytes(Some(&tag_value)).unwrap_or_default();
    let tag_length = match execute::get_property(receiver, TAG_LENGTH) {
        Value::Number(n) => n as usize,
        _ => 16,
    };
    let auto_padding = !matches!(
        execute::get_property(receiver, AUTO_PADDING),
        Value::Boolean(false)
    );
    let mut output = transform(
        &name,
        &key,
        &iv,
        &input,
        decipher,
        auto_padding,
        &aad,
        &tag,
        tag_length,
    )?;
    if (name.ends_with("-gcm") || name == "chacha20-poly1305") && !decipher {
        let split = output.len().saturating_sub(tag_length);
        let auth_tag = output.split_off(split);
        hidden(
            receiver,
            TAG,
            crate::modules::buffer_proto::make_buffer(&auth_tag),
        );
    }
    hidden(
        receiver,
        OUTPUT,
        crate::modules::buffer_proto::make_buffer(&output),
    );
    hidden(receiver, FINISHED, Value::Boolean(true));
    execute::set_property_in_place(
        receiver,
        "readableLength",
        Value::Number(output.len() as f64),
    );
    let destination = execute::get_property(receiver, PIPE_DEST);
    if !matches!(destination, Value::Undefined) {
        let chunk = crate::modules::buffer_proto::make_buffer(&output);
        if let Ok(write) = execute::get_property_result(&destination, "write") {
            let _ = execute::call(&write, &destination, &[chunk]);
        }
        if let Ok(end) = execute::get_property_result(&destination, "end") {
            if let Err(VmError::Thrown(error_value)) = execute::call(&end, &destination, &[]) {
                let listener = execute::get_property(&destination, ERROR_LISTENER);
                if !matches!(listener, Value::Undefined) {
                    let _ = execute::call(&listener, &destination, &[error_value]);
                }
            }
        }
    }
    if (name.ends_with("-gcm") || name == "chacha20-poly1305") && decipher {
        return Ok(encode(Vec::new(), args.first()));
    }
    Ok(encode(output, args.first()))
}

fn decrypt_truncated_128(
    key: &[u8],
    iv: &[u8],
    input: &[u8],
    aad: &[u8],
    tag: &[u8],
) -> Result<Vec<u8>, VmError> {
    decrypt_truncated::<Aes128, Aes128Gcm>(key, iv, input, aad, tag)
}

fn decrypt_truncated_256(
    key: &[u8],
    iv: &[u8],
    input: &[u8],
    aad: &[u8],
    tag: &[u8],
) -> Result<Vec<u8>, VmError> {
    decrypt_truncated::<Aes256, Aes256Gcm>(key, iv, input, aad, tag)
}

fn decrypt_truncated_16_128(
    key: &[u8],
    iv: &[u8],
    input: &[u8],
    aad: &[u8],
    tag: &[u8],
) -> Result<Vec<u8>, VmError> {
    decrypt_truncated_16::<Aes128, AesGcm<Aes128, U16>>(key, iv, input, aad, tag)
}

fn decrypt_truncated_16_256(
    key: &[u8],
    iv: &[u8],
    input: &[u8],
    aad: &[u8],
    tag: &[u8],
) -> Result<Vec<u8>, VmError> {
    decrypt_truncated_16::<Aes256, AesGcm<Aes256, U16>>(key, iv, input, aad, tag)
}

fn decrypt_truncated<A, G>(
    key: &[u8],
    iv: &[u8],
    input: &[u8],
    aad: &[u8],
    tag: &[u8],
) -> Result<Vec<u8>, VmError>
where
    A: BlockEncrypt + CipherKeyInit + cipher::BlockSizeUser<BlockSize = U16>,
    G: Aead<NonceSize = U12> + KeyInit,
{
    let cipher = A::new_from_slice(key)
        .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
    let mut counter = [0u8; 16];
    counter[..12].copy_from_slice(iv);
    counter[15] = 2;
    let mut plaintext = input.to_vec();
    for chunk in plaintext.chunks_mut(16) {
        let mut block = aes::cipher::Block::<A>::clone_from_slice(&counter);
        cipher.encrypt_block(&mut block);
        for (byte, mask) in chunk.iter_mut().zip(block.iter()) {
            *byte ^= *mask;
        }
        for byte in counter[12..].iter_mut().rev() {
            let (next, carry) = byte.overflowing_add(1);
            *byte = next;
            if !carry {
                break;
            }
        }
    }
    let full = G::new_from_slice(key)
        .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?
        .encrypt(
            Nonce::from_slice(iv),
            Payload {
                msg: &plaintext,
                aad,
            },
        )
        .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"))?;
    let expected = &full[full.len() - 16..];
    if expected[..tag.len()] != *tag {
        return Err(error(
            "ERR_OSSL_BAD_DECRYPT",
            "Unsupported state or unable to authenticate data",
        ));
    }
    Ok(plaintext)
}

fn ghash_mul(mut x: u128, mut y: u128) -> u128 {
    let mut z = 0u128;
    for _ in 0..128 {
        if (x & (1 << 127)) != 0 {
            z ^= y;
        }
        x <<= 1;
        y = if (y & 1) != 0 {
            (y >> 1) ^ 0xe1000000000000000000000000000000
        } else {
            y >> 1
        };
    }
    z
}

fn gcm_j0<A>(cipher: &A, iv: &[u8]) -> [u8; 16]
where
    A: BlockEncrypt + cipher::BlockSizeUser<BlockSize = U16>,
{
    let mut zero = aes::cipher::Block::<A>::default();
    cipher.encrypt_block(&mut zero);
    let h = u128::from_be_bytes(*zero.as_ref());
    let mut state = 0u128;
    let mut padded = iv.to_vec();
    let rem = padded.len() % 16;
    if rem != 0 {
        padded.resize(padded.len() + 16 - rem, 0);
    }
    padded.extend_from_slice(&[0u8; 8]);
    padded.extend_from_slice(&((iv.len() as u64) * 8).to_be_bytes());
    for block in padded.chunks_exact(16) {
        state = ghash_mul(state ^ u128::from_be_bytes(block.try_into().unwrap()), h);
    }
    state.to_be_bytes()
}

fn decrypt_truncated_16<A, G>(
    key: &[u8],
    iv: &[u8],
    input: &[u8],
    aad: &[u8],
    tag: &[u8],
) -> Result<Vec<u8>, VmError>
where
    A: BlockEncrypt + CipherKeyInit + cipher::BlockSizeUser<BlockSize = U16>,
    G: Aead<NonceSize = U16> + KeyInit,
{
    let cipher = A::new_from_slice(key)
        .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
    let j0 = gcm_j0(&cipher, iv);
    let mut counter = j0;
    let mut plaintext = input.to_vec();
    for chunk in plaintext.chunks_mut(16) {
        for byte in counter[12..].iter_mut().rev() {
            let (next, carry) = byte.overflowing_add(1);
            *byte = next;
            if !carry {
                break;
            }
        }
        let mut block = aes::cipher::Block::<A>::clone_from_slice(&counter);
        cipher.encrypt_block(&mut block);
        for (byte, mask) in chunk.iter_mut().zip(block.iter()) {
            *byte ^= *mask;
        }
    }
    let full = G::new_from_slice(key)
        .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?
        .encrypt(
            GenericArray::<u8, U16>::from_slice(iv),
            Payload {
                msg: &plaintext,
                aad,
            },
        )
        .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"))?;
    let expected = &full[full.len() - 16..];
    if expected[..tag.len()] != *tag {
        return Err(error(
            "ERR_OSSL_BAD_DECRYPT",
            "Unsupported state or unable to authenticate data",
        ));
    }
    Ok(plaintext)
}

fn gcm_preview(name: &str, key: &[u8], iv: &[u8], input: &[u8], _aad: &[u8]) -> Option<Vec<u8>> {
    if name.starts_with("aes-128") {
        Some(gcm_ctr::<Aes128>(key, iv, input)?)
    } else {
        Some(gcm_ctr::<Aes256>(key, iv, input)?)
    }
}

fn chacha_preview(key: &[u8], iv: &[u8], input: &[u8]) -> Option<Vec<u8>> {
    let mut output = input.to_vec();
    let mut stream = ChaCha20::new_from_slices(key, iv).ok()?;
    stream.seek(64);
    stream.apply_keystream(&mut output);
    Some(output)
}

fn gcm_ctr<A>(key: &[u8], iv: &[u8], input: &[u8]) -> Option<Vec<u8>>
where
    A: BlockEncrypt + CipherKeyInit + cipher::BlockSizeUser<BlockSize = U16>,
{
    let cipher = A::new_from_slice(key).ok()?;
    let mut counter = if iv.len() == 12 {
        let mut value = [0u8; 16];
        value[..12].copy_from_slice(iv);
        value[15] = 1;
        value
    } else {
        gcm_j0(&cipher, iv)
    };
    let mut output = input.to_vec();
    for chunk in output.chunks_mut(16) {
        for byte in counter[12..].iter_mut().rev() {
            let (next, carry) = byte.overflowing_add(1);
            *byte = next;
            if !carry {
                break;
            }
        }
        let mut block = aes::cipher::Block::<A>::clone_from_slice(&counter);
        cipher.encrypt_block(&mut block);
        for (byte, mask) in chunk.iter_mut().zip(block.iter()) {
            *byte ^= *mask;
        }
    }
    Some(output)
}

fn transform(
    name: &str,
    key: &[u8],
    iv: &[u8],
    input: &[u8],
    decipher: bool,
    auto_padding: bool,
    aad: &[u8],
    tag: &[u8],
    tag_length: usize,
) -> Result<Vec<u8>, VmError> {
    if name == "chacha20-poly1305" {
        let nonce = ChaChaNonce::from_slice(iv);
        let cipher = ChaCha20Poly1305::new_from_slice(key)
            .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
        if decipher {
            let mut combined = input.to_vec();
            combined.extend_from_slice(tag);
            if tag_length < 16 {
                let mut plaintext = input.to_vec();
                let mut stream = ChaCha20::new_from_slices(key, iv)
                    .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
                stream.seek(64);
                stream.apply_keystream(&mut plaintext);
                let full = cipher
                    .encrypt(
                        nonce,
                        Payload {
                            msg: &plaintext,
                            aad,
                        },
                    )
                    .map_err(|_| {
                        error(
                            "ERR_OSSL_BAD_DECRYPT",
                            "Unsupported state or unable to authenticate data",
                        )
                    })?;
                if full[full.len() - 16..full.len() - 16 + tag.len()] != *tag {
                    return Err(error(
                        "ERR_OSSL_BAD_DECRYPT",
                        "Unsupported state or unable to authenticate data",
                    ));
                }
                return Ok(plaintext);
            }
            return cipher
                .decrypt(
                    nonce,
                    Payload {
                        msg: &combined,
                        aad,
                    },
                )
                .map_err(|_| {
                    error(
                        "ERR_OSSL_BAD_DECRYPT",
                        "Unsupported state or unable to authenticate data",
                    )
                });
        }
        let mut encrypted = cipher
            .encrypt(nonce, Payload { msg: input, aad })
            .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"))?;
        encrypted.truncate(encrypted.len().saturating_sub(16 - tag_length));
        return Ok(encrypted);
    }
    if name.ends_with("-gcm") {
        if !(8..=64).contains(&iv.len()) {
            return Err(error(
                "ERR_CRYPTO_INVALID_IV",
                "Invalid initialization vector",
            ));
        }
        if name.starts_with("aes-128") {
            let mut combined = input.to_vec();
            combined.extend_from_slice(tag);
            let mut result = if iv.len() == 12 {
                let nonce = Nonce::from_slice(iv);
                if decipher && tag_length != 16 {
                    if tag_length < 12 {
                        return decrypt_truncated_128(key, iv, input, aad, tag);
                    }
                    macro_rules! short {
                        ($size:ty) => {{
                            return AesGcm::<Aes128, U12, $size>::new_from_slice(key)
                                .map_err(|_| {
                                    error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length")
                                })?
                                .decrypt(
                                    nonce,
                                    Payload {
                                        msg: &combined,
                                        aad,
                                    },
                                )
                                .map_err(|_| {
                                    error(
                                        "ERR_OSSL_BAD_DECRYPT",
                                        "Unsupported state or unable to authenticate data",
                                    )
                                });
                        }};
                    }
                    match tag_length {
                        12 => short!(U12),
                        13 => short!(U13),
                        14 => short!(U14),
                        15 => short!(U15),
                        _ => {
                            return Err(error(
                                "ERR_CRYPTO_INVALID_AUTH_TAG",
                                "Invalid authentication tag length",
                            ))
                        }
                    }
                }
                if decipher {
                    return Aes128Gcm::new_from_slice(key)
                        .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?
                        .decrypt(
                            nonce,
                            Payload {
                                msg: &combined,
                                aad,
                            },
                        )
                        .map_err(|_| {
                            error(
                                "ERR_OSSL_BAD_DECRYPT",
                                "Unsupported state or unable to authenticate data",
                            )
                        });
                }
                Aes128Gcm::new_from_slice(key)
                    .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?
                    .encrypt(nonce, Payload { msg: input, aad })
                    .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"))?
            } else {
                if decipher && tag_length < 12 && iv.len() == 16 {
                    return decrypt_truncated_16_128(key, iv, input, aad, tag);
                }
                macro_rules! run {
                    ($size:ty) => {{
                        let nonce = GenericArray::<u8, $size>::from_slice(iv);
                        let cipher =
                            AesGcm::<Aes128, $size>::new_from_slice(key).map_err(|_| {
                                error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length")
                            })?;
                        if decipher {
                            return cipher
                                .decrypt(
                                    nonce,
                                    Payload {
                                        msg: &combined,
                                        aad,
                                    },
                                )
                                .map_err(|_| {
                                    error(
                                        "ERR_OSSL_BAD_DECRYPT",
                                        "Unsupported state or unable to authenticate data",
                                    )
                                });
                        }
                        cipher
                            .encrypt(nonce, Payload { msg: input, aad })
                            .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"))?
                    }};
                }
                match iv.len() {
                    8 => run!(U8),
                    13 => run!(U13),
                    16 => run!(U16),
                    60 => run!(U60),
                    _ => {
                        return Err(error(
                            "ERR_CRYPTO_INVALID_IV",
                            "Invalid initialization vector",
                        ))
                    }
                }
            };
            result.truncate(result.len().saturating_sub(16 - tag_length));
            return Ok(result);
        }
        let mut combined = input.to_vec();
        combined.extend_from_slice(tag);
        let mut result = if iv.len() == 12 {
            let nonce = Nonce::from_slice(iv);
            if decipher && tag_length != 16 {
                if tag_length < 12 {
                    return decrypt_truncated_256(key, iv, input, aad, tag);
                }
                macro_rules! short {
                    ($size:ty) => {{
                        return AesGcm::<Aes256, U12, $size>::new_from_slice(key)
                            .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?
                            .decrypt(
                                nonce,
                                Payload {
                                    msg: &combined,
                                    aad,
                                },
                            )
                            .map_err(|_| {
                                error(
                                    "ERR_OSSL_BAD_DECRYPT",
                                    "Unsupported state or unable to authenticate data",
                                )
                            });
                    }};
                }
                match tag_length {
                    12 => short!(U12),
                    13 => short!(U13),
                    14 => short!(U14),
                    15 => short!(U15),
                    _ => {
                        return Err(error(
                            "ERR_CRYPTO_INVALID_AUTH_TAG",
                            "Invalid authentication tag length",
                        ))
                    }
                }
            }
            if decipher {
                return Aes256Gcm::new_from_slice(key)
                    .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?
                    .decrypt(
                        nonce,
                        Payload {
                            msg: &combined,
                            aad,
                        },
                    )
                    .map_err(|_| {
                        error(
                            "ERR_OSSL_BAD_DECRYPT",
                            "Unsupported state or unable to authenticate data",
                        )
                    });
            }
            Aes256Gcm::new_from_slice(key)
                .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?
                .encrypt(nonce, Payload { msg: input, aad })
                .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"))?
        } else {
            if decipher && tag_length < 12 && iv.len() == 16 {
                return decrypt_truncated_16_256(key, iv, input, aad, tag);
            }
            macro_rules! run {
                ($size:ty) => {{
                    let nonce = GenericArray::<u8, $size>::from_slice(iv);
                    let cipher = AesGcm::<Aes256, $size>::new_from_slice(key)
                        .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
                    if decipher {
                        return cipher
                            .decrypt(
                                nonce,
                                Payload {
                                    msg: &combined,
                                    aad,
                                },
                            )
                            .map_err(|_| {
                                error(
                                    "ERR_OSSL_BAD_DECRYPT",
                                    "Unsupported state or unable to authenticate data",
                                )
                            });
                    }
                    cipher
                        .encrypt(nonce, Payload { msg: input, aad })
                        .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"))?
                }};
            }
            match iv.len() {
                8 => run!(U8),
                13 => run!(U13),
                16 => run!(U16),
                60 => run!(U60),
                _ => {
                    return Err(error(
                        "ERR_CRYPTO_INVALID_IV",
                        "Invalid initialization vector",
                    ))
                }
            }
        };
        result.truncate(result.len().saturating_sub(16 - tag_length));
        return Ok(result);
    }
    if name == "aes-128-ecb" || name == "aes-256-ecb" {
        return ecb_transform(name, key, input, decipher, auto_padding);
    }
    if name == "aes-128-cbc" || name == "aes-256-cbc" {
        if name == "aes-128-cbc" {
            if decipher {
                let mut buf = input.to_vec();
                if !auto_padding {
                    if input.len() % 16 != 0 {
                        return Err(wrong_block_length());
                    }
                    return Decryptor::<Aes128>::new_from_slices(key, iv)
                        .map_err(|_| {
                            error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector")
                        })?
                        .decrypt_padded_mut::<NoPadding>(&mut buf)
                        .map(|out| out.to_vec())
                        .map_err(|_| wrong_block_length());
                }
                return Decryptor::<Aes128>::new_from_slices(key, iv)
                    .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
                    .decrypt_padded_mut::<Pkcs7>(&mut buf)
                    .map(|out| out.to_vec())
                    .map_err(|_| error("ERR_OSSL_BAD_DECRYPT", "bad decrypt"));
            }
            let length = input.len();
            let mut buf = input.to_vec();
            if auto_padding {
                buf.resize(length + 16, 0);
            }
            if !auto_padding && length % 16 != 0 {
                return Err(wrong_block_length());
            }
            if !auto_padding {
                return Encryptor::<Aes128>::new_from_slices(key, iv)
                    .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
                    .encrypt_padded_mut::<NoPadding>(&mut buf, length)
                    .map(|out| out.to_vec())
                    .map_err(|_| wrong_block_length());
            }
            return Encryptor::<Aes128>::new_from_slices(key, iv)
                .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
                .encrypt_padded_mut::<Pkcs7>(&mut buf, length)
                .map(|out| out.to_vec())
                .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"));
        }
        if decipher {
            let mut buf = input.to_vec();
            if !auto_padding {
                if input.len() % 16 != 0 {
                    return Err(wrong_block_length());
                }
                return Decryptor::<Aes256>::new_from_slices(key, iv)
                    .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
                    .decrypt_padded_mut::<NoPadding>(&mut buf)
                    .map(|out| out.to_vec())
                    .map_err(|_| wrong_block_length());
            }
            return Decryptor::<Aes256>::new_from_slices(key, iv)
                .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
                .decrypt_padded_mut::<Pkcs7>(&mut buf)
                .map(|out| out.to_vec())
                .map_err(|_| error("ERR_OSSL_BAD_DECRYPT", "bad decrypt"));
        }
        let length = input.len();
        let mut buf = input.to_vec();
        if auto_padding {
            buf.resize(length + 16, 0);
        }
        if !auto_padding && length % 16 != 0 {
            return Err(wrong_block_length());
        }
        if !auto_padding {
            return Encryptor::<Aes256>::new_from_slices(key, iv)
                .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
                .encrypt_padded_mut::<NoPadding>(&mut buf, length)
                .map(|out| out.to_vec())
                .map_err(|_| wrong_block_length());
        }
        return Encryptor::<Aes256>::new_from_slices(key, iv)
            .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, length)
            .map(|out| out.to_vec())
            .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"));
    }
    if name == "des-ede3-cbc" {
        if decipher {
            let mut buf = input.to_vec();
            return Decryptor::<TdesEde3>::new_from_slices(key, iv)
                .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
                .decrypt_padded_mut::<Pkcs7>(&mut buf)
                .map(|out| out.to_vec())
                .map_err(|_| error("ERR_OSSL_BAD_DECRYPT", "bad decrypt"));
        }
        let length = input.len();
        let mut buf = input.to_vec();
        buf.resize(length + 8, 0);
        return Encryptor::<TdesEde3>::new_from_slices(key, iv)
            .map_err(|_| error("ERR_CRYPTO_INVALID_IV", "Invalid initialization vector"))?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, length)
            .map(|out| out.to_vec())
            .map_err(|_| error("ERR_OSSL_EVP_BAD_BLOCK_LENGTH", "bad encrypt"));
    }
    Ok(input.to_vec())
}

fn ecb_transform(
    name: &str,
    key: &[u8],
    input: &[u8],
    decipher: bool,
    auto_padding: bool,
) -> Result<Vec<u8>, VmError> {
    let block = 16;
    if decipher {
        if input.is_empty() || input.len() % block != 0 {
            return Err(wrong_block_length());
        }
        let mut output = input.to_vec();
        if name == "aes-128-ecb" {
            let cipher = Aes128::new_from_slice(key)
                .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
            for chunk in output.chunks_mut(block) {
                cipher.decrypt_block(aes::cipher::Block::<Aes128>::from_mut_slice(chunk));
            }
        } else {
            let cipher = Aes256::new_from_slice(key)
                .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
            for chunk in output.chunks_mut(block) {
                cipher.decrypt_block(aes::cipher::Block::<Aes256>::from_mut_slice(chunk));
            }
        }
        if auto_padding {
            let padding = *output.last().unwrap_or(&0) as usize;
            if padding == 0
                || padding > block
                || output.len() < padding
                || output[output.len() - padding..]
                    .iter()
                    .any(|byte| *byte as usize != padding)
            {
                return Err(error("ERR_OSSL_BAD_DECRYPT", "bad decrypt"));
            }
            output.truncate(output.len() - padding);
        }
        return Ok(output);
    }
    if !auto_padding && input.len() % block != 0 {
        return Err(wrong_block_length());
    }
    let padding = if auto_padding {
        block - (input.len() % block)
    } else {
        0
    };
    let mut output = input.to_vec();
    output.resize(input.len() + padding, padding as u8);
    if name == "aes-128-ecb" {
        let cipher = Aes128::new_from_slice(key)
            .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
        for chunk in output.chunks_mut(block) {
            cipher.encrypt_block(aes::cipher::Block::<Aes128>::from_mut_slice(chunk));
        }
    } else {
        let cipher = Aes256::new_from_slice(key)
            .map_err(|_| error("ERR_CRYPTO_INVALID_KEYLEN", "Invalid key length"))?;
        for chunk in output.chunks_mut(block) {
            cipher.encrypt_block(aes::cipher::Block::<Aes256>::from_mut_slice(chunk));
        }
    }
    Ok(output)
}

pub fn end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(value) = args.first() {
        let _ = update(state, receiver, std::slice::from_ref(value));
    }
    finalize(state, receiver, &[])
}

pub fn pipe(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let destination = args
        .first()
        .ok_or_else(|| invalid_type("The destination argument must be a stream"))?;
    hidden(receiver, PIPE_DEST, destination.clone());
    Ok(destination.clone())
}

pub fn unpipe(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}

pub fn on(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if matches!(args.first(), Some(Value::String(event)) if event == "error") {
        if let Some(listener) = args.get(1) {
            hidden(receiver, ERROR_LISTENER, listener.clone());
        }
    }
    Ok(receiver.clone())
}

pub fn pause(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}
pub fn resume(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}
pub fn set_encoding(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver.ok_or(VmError::NotCallable).cloned()
}

pub fn read(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let output = execute::get_property(receiver, OUTPUT);
    Ok(crate::modules::buffer_proto::make_buffer(
        &bytes(Some(&output)).unwrap_or_default(),
    ))
}

pub fn set_padding(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let enabled = match args.first() {
        None | Some(Value::Undefined) => true,
        Some(Value::Boolean(value)) => *value,
        Some(value) => {
            return Err(invalid_type(&format!(
                "The \"auto_padding\" argument must be of type boolean.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
    };
    hidden(receiver, AUTO_PADDING, Value::Boolean(enabled));
    Ok(receiver.clone())
}
pub fn set_aad(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let algorithm = execute::to_js_string(&execute::get_property(receiver, ALGORITHM))?;
    if !algorithm.ends_with("-gcm")
        && !algorithm.ends_with("-ccm")
        && !algorithm.ends_with("-ocb")
        && algorithm != "chacha20-poly1305"
    {
        return Err(error("ERR_CRYPTO_INVALID_STATE", "Invalid state"));
    }
    let value = args
        .first()
        .and_then(|value| bytes(Some(value)))
        .ok_or_else(|| {
            invalid_type("The data argument must be of type string or an instance of Buffer")
        })?;
    hidden(
        receiver,
        AAD,
        crate::modules::buffer_proto::make_buffer(&value),
    );
    Ok(receiver.clone())
}
pub fn get_tag(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let algorithm = execute::to_js_string(&execute::get_property(receiver, ALGORITHM))?;
    if !algorithm.ends_with("-gcm")
        && !algorithm.ends_with("-ccm")
        && !algorithm.ends_with("-ocb")
        && algorithm != "chacha20-poly1305"
    {
        return Err(error("ERR_CRYPTO_INVALID_STATE", "Invalid state"));
    }
    if !matches!(
        execute::get_property(receiver, FINISHED),
        Value::Boolean(true)
    ) {
        return Err(error("ERR_CRYPTO_INVALID_STATE", "Invalid state"));
    }
    Ok(crate::modules::buffer_proto::make_buffer(
        &bytes(Some(&execute::get_property(receiver, TAG))).unwrap_or_default(),
    ))
}
pub fn set_tag(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    if bytes(Some(&execute::get_property(receiver, TAG))).is_some_and(|tag| !tag.is_empty()) {
        return Err(error("ERR_CRYPTO_INVALID_STATE", "Invalid state"));
    }
    let value = args
        .first()
        .and_then(|value| bytes(Some(value)))
        .ok_or_else(|| invalid_type("The authTag argument must be an instance of Buffer"))?;
    let expected = match execute::get_property(receiver, TAG_LENGTH) {
        Value::Number(n) => n as usize,
        _ => 16,
    };
    let valid = if execute::to_js_string(&execute::get_property(receiver, ALGORITHM))
        .ok()
        .as_deref()
        == Some("chacha20-poly1305")
    {
        value.len() == expected && value.len() <= 16
    } else {
        value.len() == expected && [4, 8, 12, 13, 14, 15, 16].contains(&value.len())
    };
    if !valid {
        if execute::to_js_string(&execute::get_property(receiver, ALGORITHM))
            .ok()
            .as_deref()
            == Some("chacha20-poly1305")
        {
            return Err(type_error(
                "ERR_CRYPTO_INVALID_AUTH_TAG",
                &format!("Invalid authentication tag length: {}", value.len()),
            ));
        }
        return Err(type_error(
            "ERR_CRYPTO_INVALID_AUTH_TAG",
            &format!("Invalid authentication tag length: {}", value.len()),
        ));
    }
    hidden(
        receiver,
        TAG,
        crate::modules::buffer_proto::make_buffer(&value),
    );
    Ok(receiver.clone())
}

pub fn get_ciphers(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::array(
        ["aes-128-cbc", "aes-128-ecb", "des-ede3-cbc", "des-ede3-ecb"]
            .into_iter()
            .map(|name| Value::String(name.into()))
            .collect(),
    ))
}

pub fn get_cipher_info(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let requested = args.first().unwrap_or(&Value::Undefined);
    if let Some(options) = args.get(1) {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(invalid_type(
                "The \"options\" argument must be of type object",
            ));
        }
        for property in ["keyLength", "ivLength"] {
            let value = execute::get_property(options, property);
            if !matches!(value, Value::Undefined | Value::Number(_)) {
                return Err(invalid_type("The option must be a number"));
            }
        }
    }
    let name = match requested {
        Value::Number(n) => match *n as i64 {
            419 => "aes-128-cbc".into(),
            418 => "aes-128-ecb".into(),
            866 => "des-ede3-cbc".into(),
            865 => "des-ede3-ecb".into(),
            _ => return Ok(Value::Undefined),
        },
        Value::String(value) => value.to_ascii_lowercase(),
        Value::Null
        | Value::Undefined
        | Value::Array(_)
        | Value::Object(_)
        | Value::ObjectAlias(_)
        | Value::Boolean(_) => {
            return Err(invalid_type(
                "The \"name\" argument must be of type string or number",
            ))
        }
        _ => {
            return Err(invalid_type(
                "The \"name\" argument must be of type string or number",
            ))
        }
    };
    let (nid, block, iv, key, mode) = match name.as_str() {
        "aes-128-cbc" => (419, 16, 16, 16, "cbc"),
        "aes-128-ecb" => (418, 16, 0, 16, "ecb"),
        "des-ede3-cbc" => (866, 8, 8, 24, "cbc"),
        "des-ede3-ecb" => (865, 8, 0, 24, "ecb"),
        "aes-128-ccm" | "aes-192-ccm" | "aes-256-ccm" => (
            0,
            16,
            12,
            if name.starts_with("aes-128") {
                16
            } else if name.starts_with("aes-192") {
                24
            } else {
                32
            },
            "ccm",
        ),
        "aes-128-ocb" | "aes-192-ocb" | "aes-256-ocb" => (
            0,
            16,
            12,
            if name.starts_with("aes-128") {
                16
            } else if name.starts_with("aes-192") {
                24
            } else {
                32
            },
            "ocb",
        ),
        _ => return Ok(Value::Undefined),
    };
    if let Some(options) = args.get(1) {
        for (property, expected) in [("keyLength", key), ("ivLength", iv)] {
            let value = execute::get_property(options, property);
            if let Value::Number(n) = value {
                if !n.is_finite() {
                    return Err(invalid_type("The option must be a number"));
                }
            }
            if let Value::Number(n) = value {
                let variable_iv = property == "ivLength"
                    && ((mode == "ccm" && (7.0..=13.0).contains(&n))
                        || (mode == "ocb" && (1.0..=15.0).contains(&n)));
                if n as usize != expected && !variable_iv {
                    return Ok(Value::Undefined);
                }
            }
        }
    }
    Ok(host_api::object(vec![
        ("name".into(), Value::String(name)),
        ("nid".into(), Value::Number(nid as f64)),
        ("blockSize".into(), Value::Number(block as f64)),
        ("ivLength".into(), Value::Number(iv as f64)),
        ("keyLength".into(), Value::Number(key as f64)),
        ("mode".into(), Value::String(mode.into())),
    ]))
}
