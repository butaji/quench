//! Rust-owned WebCrypto boundary.
//!
//! The engine owns promises and typed-array storage; this module owns the
//! observable WebCrypto facts that do not require a separate JS runtime.

use std::cell::RefCell;
use std::rc::Rc;

use aes::{Aes128, Aes192, Aes256};
use aes_gcm::{
    aead::{Aead, Payload},
    KeyInit,
};
use base64::Engine;
use chacha20poly1305::ChaCha20Poly1305;
use cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt};
use hmac::{Hmac, Mac};
use p384::elliptic_curve::sec1::ToEncodedPoint as P384ToEncodedPoint;
use p384::{
    ecdh::diffie_hellman as p384_diffie_hellman, PublicKey as P384PublicKey,
    SecretKey as P384SecretKey,
};
use p521::elliptic_curve::sec1::ToSec1Point as P521ToSec1Point;
use p521::{
    ecdh::diffie_hellman as p521_diffie_hellman, PublicKey as P521PublicKey,
    SecretKey as P521SecretKey,
};
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::ops::Builtin;
use quench_runtime::value::{ArrayBufferData, PromiseData, PromiseState, Value};
use rand::RngCore;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};
use sha3::{
    digest::ExtendableOutput, digest::Update as ShaUpdate, digest::XofReader, Sha3_256, Sha3_384,
    Sha3_512, TurboShake128, TurboShake128Core, TurboShake256, TurboShake256Core,
};
use tiny_keccak::{CShake, Hasher as TinyHasher};

use crate::host::HostState;

pub(crate) const KEY_MARKER_PROP: &str = "\0quench:webcrypto:key";
pub(crate) const KEY_DATA_PROP: &str = "\0quench:webcrypto:key-data";
pub(crate) const KEY_FORMAT_PROP: &str = "\0quench:webcrypto:key-format";
pub(crate) const KEY_META_PROP: &str = "\0quench:webcrypto:key-meta";
const KEY_JWK_PROP: &str = "\0quench:webcrypto:jwk";
const KEY_PUBLIC_ALGORITHM_PROP: &str = "\0quench:webcrypto:public-algorithm";
const KEY_PUBLIC_USAGES_PROP: &str = "\0quench:webcrypto:public-usages";

thread_local! {
    // CryptoKey instances can be created while a promise continuation is
    // running with a sandbox/worker global as the current VM realm.  The
    // constructor's prototype is a host fact owned by the root context, so
    // retain that identity once during module construction instead of
    // resolving a realm-local marker for every operation.
    static KEY_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

fn key_prototype() -> Value {
    KEY_PROTOTYPE.with(|prototype| {
        prototype.borrow().clone().unwrap_or_else(|| {
            let global = quench_runtime::vm::current_global_object();
            execute::get_property(&global, "__quench_crypto_key_prototype")
        })
    })
}

fn settled(result: Result<Value, VmError>) -> Value {
    Value::Promise(Rc::new(PromiseData::new(match result {
        Ok(value) => PromiseState::Fulfilled(value),
        Err(VmError::Thrown(value)) => PromiseState::Rejected(value),
        Err(_) => PromiseState::Rejected(Value::String("Operation failed".into())),
    })))
}

fn invalid_subtle_this(receiver: Option<&Value>) -> Option<Value> {
    let valid = receiver.is_some_and(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            && quench_runtime::is_callable(&execute::get_property(value, "digest"))
    });
    (!valid).then(|| {
        settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Value of \"this\" must be of type SubtleCrypto",
        )))
    })
}

fn error(kind: Builtin, code: Option<&str>, message: &str) -> VmError {
    let value = quench_runtime::builtins::error(kind, &[Value::String(message.into())]);
    let value = code.map_or(value.clone(), |code| {
        execute::set_property(value, "code", Value::String(code.into()))
    });
    VmError::Thrown(value)
}

pub fn illegal_constructor(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(error(
        Builtin::TypeError,
        Some("ERR_ILLEGAL_CONSTRUCTOR"),
        "Illegal constructor",
    ))
}

fn bytes(value: &Value) -> Option<Vec<u8>> {
    macro_rules! view {
        ($view:expr) => {{
            if $view.buffer.shared {
                return None;
            }
            let bytes = $view.buffer.bytes.borrow();
            Some(
                bytes
                    .get($view.byte_offset..$view.byte_offset + $view.byte_length())?
                    .to_vec(),
            )
        }};
    }
    match value {
        Value::ArrayBuffer(buffer) if !buffer.shared => Some(buffer.bytes.borrow().clone()),
        Value::DataView(view) => view!(view),
        Value::Float64Array(view) => view!(view),
        Value::Float32Array(view) => view!(view),
        Value::Int8Array(view) => view!(view),
        Value::Int16Array(view) => view!(view),
        Value::Int32Array(view) => view!(view),
        Value::BigInt64Array(view) => view!(view),
        Value::BigUint64Array(view) => view!(view),
        Value::Uint8Array(view) => view!(view),
        Value::Uint8ClampedArray(view) => view!(view),
        Value::Uint16Array(view) => view!(view),
        Value::Uint32Array(view) => view!(view),
        _ => None,
    }
}

fn array_buffer(data: &[u8]) -> Value {
    let buffer = Rc::new(ArrayBufferData::new(data.len()));
    buffer.bytes.borrow_mut().copy_from_slice(data);
    Value::ArrayBuffer(buffer)
}

fn turbo_shake(input: &[u8], domain: u8, output_len: usize, is_256: bool) -> Vec<u8> {
    if is_256 {
        let mut hasher = TurboShake256::from_core(TurboShake256Core::new(domain));
        ShaUpdate::update(&mut hasher, input);
        let mut reader = hasher.finalize_xof();
        let mut output = vec![0; output_len];
        reader.read(&mut output);
        output
    } else {
        let mut hasher = TurboShake128::from_core(TurboShake128Core::new(domain));
        ShaUpdate::update(&mut hasher, input);
        let mut reader = hasher.finalize_xof();
        let mut output = vec![0; output_len];
        reader.read(&mut output);
        output
    }
}

// RFC 9861 section 3.3: the integer is encoded big-endian, followed by the
// number of bytes used for the integer (with zero represented as one byte).
fn length_encode(value: usize) -> Vec<u8> {
    if value == 0 {
        return vec![0];
    }
    let mut encoded = Vec::new();
    let mut remaining = value;
    while remaining != 0 {
        encoded.push((remaining & 0xff) as u8);
        remaining >>= 8;
    }
    encoded.reverse();
    encoded.push(encoded.len() as u8);
    encoded
}

// KangarooTwelve is specified in terms of TurboSHAKE, rather than the
// KT128-only helper exposed by tiny-keccak.  Keeping the tree construction
// here gives KT128 and KT256 the same complete RFC 9861 semantics.
fn kangaroo_twelve(
    message: &[u8],
    customization: &[u8],
    output_len: usize,
    is_256: bool,
) -> Vec<u8> {
    const CHUNK_SIZE: usize = 8192;
    let customization_length = length_encode(customization.len());
    let mut input = Vec::with_capacity(
        message
            .len()
            .saturating_add(customization.len())
            .saturating_add(customization_length.len()),
    );
    input.extend_from_slice(message);
    input.extend_from_slice(customization);
    input.extend_from_slice(&customization_length);

    if input.len() <= CHUNK_SIZE {
        return turbo_shake(&input, 0x07, output_len, is_256);
    }

    let cv_len = if is_256 { 64 } else { 32 };
    let mut final_node = Vec::with_capacity(input.len() / CHUNK_SIZE * cv_len + CHUNK_SIZE + 32);
    final_node.extend_from_slice(&input[..CHUNK_SIZE]);
    final_node.push(0x03);
    final_node.extend_from_slice(&[0; 7]);

    let mut block_count = 0;
    for chunk in input[CHUNK_SIZE..].chunks(CHUNK_SIZE) {
        let cv = turbo_shake(chunk, 0x0b, cv_len, is_256);
        final_node.extend_from_slice(&cv);
        block_count += 1;
    }
    final_node.extend_from_slice(&length_encode(block_count));
    final_node.extend_from_slice(&[0xff, 0xff]);
    turbo_shake(&final_node, 0x06, output_len, is_256)
}

// X25519 is kept here as a small, self-contained Montgomery ladder so the
// WebCrypto boundary does not need to delegate key agreement to the OpenSSL
// backed `crypto` module.  The representation is radix 2^51; every product
// fits in u128 and the ladder follows RFC 7748 section 5.
#[derive(Clone, Copy)]
struct X25519Field([u64; 5]);

const X25519_LIMB_MASK: u64 = (1_u64 << 51) - 1;
const X25519_P: [u64; 5] = [
    X25519_LIMB_MASK - 18,
    X25519_LIMB_MASK,
    X25519_LIMB_MASK,
    X25519_LIMB_MASK,
    X25519_LIMB_MASK,
];

impl X25519Field {
    const ZERO: Self = Self([0; 5]);
    const ONE: Self = Self([1, 0, 0, 0, 0]);

    fn reduce(mut limbs: [u128; 5]) -> Self {
        for _ in 0..2 {
            let mut carry = 0_u128;
            for limb in &mut limbs {
                *limb += carry;
                carry = *limb >> 51;
                *limb &= X25519_LIMB_MASK as u128;
            }
            limbs[0] += carry * 19;
        }
        Self(limbs.map(|limb| limb as u64))
    }

    fn add(self, rhs: Self) -> Self {
        Self::reduce(std::array::from_fn(|index| {
            self.0[index] as u128 + rhs.0[index] as u128
        }))
    }

    fn sub(self, rhs: Self) -> Self {
        Self::reduce(std::array::from_fn(|index| {
            self.0[index] as u128 + 2 * X25519_P[index] as u128 - rhs.0[index] as u128
        }))
    }

    fn mul(self, rhs: Self) -> Self {
        let mut product = [0_u128; 9];
        for left in 0..5 {
            for right in 0..5 {
                product[left + right] += self.0[left] as u128 * rhs.0[right] as u128;
            }
        }
        for index in (5..9).rev() {
            product[index - 5] += product[index] * 19;
        }
        Self::reduce(product[..5].try_into().expect("fixed X25519 product"))
    }

    fn square(self) -> Self {
        self.mul(self)
    }

    fn canonical(self) -> Self {
        let mut candidate = [0_u64; 5];
        let mut borrow = 0_i128;
        for index in 0..5 {
            let value = self.0[index] as i128 - X25519_P[index] as i128 - borrow;
            candidate[index] = (value.rem_euclid(1_i128 << 51)) as u64;
            borrow = i128::from(value < 0);
        }
        if borrow == 0 {
            Self(candidate)
        } else {
            self
        }
    }

    fn invert(self) -> Self {
        // p - 2 = 2^255 - 21, represented little-endian.
        let mut result = Self::ONE;
        let exponent_low = 0xeb_u8;
        for bit in (0..255).rev() {
            result = result.square();
            let set = if bit < 8 {
                (exponent_low >> bit) & 1
            } else {
                1
            };
            if set != 0 {
                result = result.mul(self);
            }
        }
        result
    }

    fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut limbs = [0_u64; 5];
        for bit in 0..255 {
            limbs[bit / 51] |= u64::from((bytes[bit / 8] >> (bit % 8)) & 1) << (bit % 51);
        }
        Self(limbs).canonical()
    }

    fn to_bytes(self) -> [u8; 32] {
        let value = self.canonical();
        let mut bytes = [0_u8; 32];
        for bit in 0..255 {
            bytes[bit / 8] |= (((value.0[bit / 51] >> (bit % 51)) & 1) as u8) << (bit % 8);
        }
        bytes
    }
}

fn x25519(scalar: &[u8; 32], u_coordinate: &[u8; 32]) -> [u8; 32] {
    let mut scalar = *scalar;
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;
    let mut u = *u_coordinate;
    u[31] &= 127;

    let x1 = X25519Field::from_bytes(&u);
    let mut x2 = X25519Field::ONE;
    let mut z2 = X25519Field::ZERO;
    let mut x3 = x1;
    let mut z3 = X25519Field::ONE;
    let mut swap = 0_u64;
    for bit in (0..255).rev() {
        let bit_value = u64::from((scalar[bit / 8] >> (bit % 8)) & 1);
        swap ^= bit_value;
        let mask = 0_u64.wrapping_sub(swap);
        for index in 0..5 {
            let difference = mask & (x2.0[index] ^ x3.0[index]);
            x2.0[index] ^= difference;
            x3.0[index] ^= difference;
            let difference = mask & (z2.0[index] ^ z3.0[index]);
            z2.0[index] ^= difference;
            z3.0[index] ^= difference;
        }
        swap = bit_value;

        let a = x2.add(z2);
        let aa = a.square();
        let b = x2.sub(z2);
        let bb = b.square();
        let e = aa.sub(bb);
        let c = x3.add(z3);
        let d = x3.sub(z3);
        let da = d.mul(a);
        let cb = c.mul(b);
        x3 = da.add(cb).square();
        z3 = x1.mul(da.sub(cb).square());
        x2 = aa.mul(bb);
        z2 = e.mul(aa.add(X25519Field([121665, 0, 0, 0, 0]).mul(e)));
    }
    let mask = 0_u64.wrapping_sub(swap);
    for index in 0..5 {
        let difference = mask & (x2.0[index] ^ x3.0[index]);
        x2.0[index] ^= difference;
        x3.0[index] ^= difference;
        let difference = mask & (z2.0[index] ^ z3.0[index]);
        z2.0[index] ^= difference;
        z3.0[index] ^= difference;
    }
    x2.mul(z2.invert()).to_bytes()
}

#[derive(Clone, Copy)]
struct X448Field([u64; 8]);

const X448_LIMB_MASK: u64 = (1_u64 << 56) - 1;
const X448_P: [u64; 8] = [
    X448_LIMB_MASK,
    X448_LIMB_MASK,
    X448_LIMB_MASK,
    X448_LIMB_MASK,
    X448_LIMB_MASK - 1,
    X448_LIMB_MASK,
    X448_LIMB_MASK,
    X448_LIMB_MASK,
];

impl X448Field {
    const ZERO: Self = Self([0; 8]);
    const ONE: Self = Self([1, 0, 0, 0, 0, 0, 0, 0]);

    fn reduce(mut limbs: [u128; 8]) -> Self {
        for _ in 0..2 {
            let mut carry = 0_u128;
            for limb in &mut limbs {
                *limb += carry;
                carry = *limb >> 56;
                *limb &= X448_LIMB_MASK as u128;
            }
            limbs[0] += carry;
            limbs[4] += carry;
        }
        Self(limbs.map(|limb| limb as u64))
    }

    fn add(self, rhs: Self) -> Self {
        Self::reduce(std::array::from_fn(|index| {
            self.0[index] as u128 + rhs.0[index] as u128
        }))
    }

    fn sub(self, rhs: Self) -> Self {
        Self::reduce(std::array::from_fn(|index| {
            self.0[index] as u128 + 2 * X448_P[index] as u128 - rhs.0[index] as u128
        }))
    }

    fn mul(self, rhs: Self) -> Self {
        let mut product = [0_u128; 15];
        for left in 0..8 {
            for right in 0..8 {
                product[left + right] += self.0[left] as u128 * rhs.0[right] as u128;
            }
        }
        for index in (8..15).rev() {
            product[index - 8] += product[index];
            product[index - 4] += product[index];
        }
        Self::reduce(product[..8].try_into().expect("fixed X448 product"))
    }

    fn square(self) -> Self {
        self.mul(self)
    }

    fn canonical(self) -> Self {
        let mut candidate = [0_u64; 8];
        let mut borrow = 0_i128;
        for index in 0..8 {
            let value = self.0[index] as i128 - X448_P[index] as i128 - borrow;
            candidate[index] = (value.rem_euclid(1_i128 << 56)) as u64;
            borrow = i128::from(value < 0);
        }
        if borrow == 0 {
            Self(candidate)
        } else {
            self
        }
    }

    fn invert(self) -> Self {
        // p - 2 = 2^448 - 2^224 - 3.  Its little-endian bytes are
        // [0xfd, ff..ff, 0xfe, ff..ff].
        let mut result = Self::ONE;
        for bit in (0..448).rev() {
            result = result.square();
            let set = if bit < 8 {
                (0xfd_u8 >> bit) & 1
            } else if bit < 224 {
                1
            } else if bit == 224 {
                0
            } else if bit < 232 {
                (0xfe_u8 >> (bit - 224)) & 1
            } else {
                1
            };
            if set != 0 {
                result = result.mul(self);
            }
        }
        result
    }

    fn from_bytes(bytes: &[u8; 56]) -> Self {
        let mut limbs = [0_u64; 8];
        for bit in 0..448 {
            limbs[bit / 56] |= u64::from((bytes[bit / 8] >> (bit % 8)) & 1) << (bit % 56);
        }
        Self(limbs).canonical()
    }

    fn to_bytes(self) -> [u8; 56] {
        let value = self.canonical();
        let mut bytes = [0_u8; 56];
        for bit in 0..448 {
            bytes[bit / 8] |= (((value.0[bit / 56] >> (bit % 56)) & 1) as u8) << (bit % 8);
        }
        bytes
    }
}

fn x448(scalar: &[u8; 56], u_coordinate: &[u8; 56]) -> [u8; 56] {
    let mut scalar = *scalar;
    scalar[0] &= 252;
    scalar[55] |= 128;

    let x1 = X448Field::from_bytes(u_coordinate);
    let mut x2 = X448Field::ONE;
    let mut z2 = X448Field::ZERO;
    let mut x3 = x1;
    let mut z3 = X448Field::ONE;
    let mut swap = 0_u64;
    for bit in (0..448).rev() {
        let bit_value = u64::from((scalar[bit / 8] >> (bit % 8)) & 1);
        swap ^= bit_value;
        let mask = 0_u64.wrapping_sub(swap);
        for index in 0..8 {
            let difference = mask & (x2.0[index] ^ x3.0[index]);
            x2.0[index] ^= difference;
            x3.0[index] ^= difference;
            let difference = mask & (z2.0[index] ^ z3.0[index]);
            z2.0[index] ^= difference;
            z3.0[index] ^= difference;
        }
        swap = bit_value;

        let a = x2.add(z2);
        let aa = a.square();
        let b = x2.sub(z2);
        let bb = b.square();
        let e = aa.sub(bb);
        let c = x3.add(z3);
        let d = x3.sub(z3);
        let da = d.mul(a);
        let cb = c.mul(b);
        x3 = da.add(cb).square();
        z3 = x1.mul(da.sub(cb).square());
        x2 = aa.mul(bb);
        z2 = e.mul(aa.add(X448Field([39081, 0, 0, 0, 0, 0, 0, 0]).mul(e)));
    }
    let mask = 0_u64.wrapping_sub(swap);
    for index in 0..8 {
        let difference = mask & (x2.0[index] ^ x3.0[index]);
        x2.0[index] ^= difference;
        x3.0[index] ^= difference;
        let difference = mask & (z2.0[index] ^ z3.0[index]);
        z2.0[index] ^= difference;
        z3.0[index] ^= difference;
    }
    x2.mul(z2.invert()).to_bytes()
}

fn key_slot(key: &Value, name: &str) -> Value {
    let value = execute::get_property(key, name);
    if matches!(value, Value::Undefined) {
        let metadata = execute::get_property(key, KEY_META_PROP);
        execute::get_property(&metadata, name)
    } else {
        value
    }
}

fn cfrg_key_bytes(key: &Value, private: bool) -> Option<Vec<u8>> {
    let data = bytes(&execute::get_property(key, KEY_DATA_PROP))?;
    if matches!(data.len(), 32 | 56) {
        return Some(data);
    }
    let markers: &[&[u8]] = if private {
        &[&[0x04, 0x22, 0x04, 0x20], &[0x04, 0x3a, 0x04, 0x38]]
    } else {
        &[&[0x03, 0x21, 0x00], &[0x03, 0x39, 0x00]]
    };
    for marker in markers {
        let Some(position) = data
            .windows(marker.len())
            .position(|window| window == *marker)
        else {
            continue;
        };
        let start = position + marker.len();
        let size = if marker.len() == 4 {
            marker[3] as usize
        } else {
            marker[1] as usize - 1
        };
        let end = start.checked_add(size)?;
        if let Some(bytes) = data.get(start..end) {
            return Some(bytes.to_vec());
        }
    }
    None
}

fn cfrg_derive_bits(
    algorithm: &Value,
    base_key: &Value,
    length: Option<&Value>,
) -> Result<Vec<u8>, VmError> {
    let public = execute::get_property(algorithm, "public");
    if matches!(public, Value::Undefined) {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_MISSING_OPTION"),
            "The \"public\" option is required",
        ));
    }
    if invalid_key_this(&public).is_some() {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The public key must be a CryptoKey",
        ));
    }
    let public_type = execute::to_js_string(&key_slot(&public, "type")).unwrap_or_default();
    if public_type != "public" {
        return Err(invalid_access_error("Unable to use this key to deriveBits"));
    }
    let public_algorithm = key_slot(&public, "algorithm");
    let requested_name = algorithm_name(algorithm);
    let public_name = algorithm_name(&public_algorithm);
    if !requested_name.eq_ignore_ascii_case(&public_name) {
        return Err(operation_error("key algorithm mismatch"));
    }
    let length = match length {
        Some(Value::Number(value))
            if value.is_finite() && value.fract() == 0.0 && *value >= 0.0 =>
        {
            if *value > i32::MAX as f64 {
                return Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ));
            }
            *value as usize
        }
        Some(Value::Null | Value::Undefined) | None => {
            if requested_name.eq_ignore_ascii_case("X448") {
                448
            } else {
                256
            }
        }
        Some(_) => {
            return Err(error(
                Builtin::TypeError,
                Some("ERR_INVALID_ARG_TYPE"),
                "The length must be a number",
            ))
        }
    };
    let size = if requested_name.eq_ignore_ascii_case("X448") {
        56
    } else {
        32
    };
    if length > size * 8 {
        return Err(operation_error("derived bit length is too small"));
    }
    let private = cfrg_key_bytes(base_key, true)
        .ok_or_else(|| operation_error("Invalid private key data"))?;
    let public =
        cfrg_key_bytes(&public, false).ok_or_else(|| operation_error("Invalid public key data"))?;
    if private.len() != size || public.len() != size {
        return Err(operation_error("Key data has the wrong length"));
    }
    let secret = if size == 32 {
        x25519(
            &private.try_into().expect("validated X25519 private length"),
            &public.try_into().expect("validated X25519 public length"),
        )
        .to_vec()
    } else {
        x448(
            &private.try_into().expect("validated X448 private length"),
            &public.try_into().expect("validated X448 public length"),
        )
        .to_vec()
    };
    if secret.iter().all(|byte| *byte == 0) {
        return Err(operation_error("Invalid public key"));
    }
    let output_length = length.div_ceil(8);
    let mut output = secret[..output_length].to_vec();
    if let remainder @ 1..=7 = length % 8 {
        let mask = 0xff_u8 << (8 - remainder);
        if let Some(last) = output.last_mut() {
            *last &= mask;
        }
    }
    Ok(output)
}

fn ec_key_bytes(key: &Value, private: bool, size: usize) -> Option<Vec<u8>> {
    let data = bytes(&execute::get_property(key, KEY_DATA_PROP))?;
    if private {
        if data.len() == size {
            return Some(data);
        }
        // PKCS#8 wraps an ECPrivateKey in an OCTET STRING.  The scalar is
        // the first OCTET STRING with the curve's field width; looking for
        // that typed value also keeps the parser independent of DER length
        // encoding details used by P-384 and P-521.
        let marker = u8::try_from(size).ok()?;
        for position in 0..data.len().saturating_sub(size + 1) {
            if data[position] == 0x04 && data[position + 1] == marker {
                return Some(data[position + 2..position + 2 + size].to_vec());
            }
        }
        None
    } else {
        if data.len() == 1 + size * 2 && data.first() == Some(&0x04) {
            return Some(data);
        }
        // SPKI's BIT STRING contains an uncompressed SEC1 point: 00 04 X Y.
        // Return the complete SEC1 point expected by the RustCrypto parser.
        let point_size = 1 + size * 2;
        data.windows(point_size)
            .enumerate()
            .position(|(position, window)| {
                position > 0 && data[position - 1] == 0 && window[0] == 0x04
            })
            .map(|position| data[position..position + point_size].to_vec())
    }
}

fn ecdh_derive_bits(
    algorithm: &Value,
    base_key: &Value,
    length: Option<&Value>,
) -> Result<Vec<u8>, VmError> {
    let public = execute::get_property(algorithm, "public");
    if matches!(public, Value::Undefined) {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_MISSING_OPTION"),
            "The \"public\" option is required",
        ));
    }
    if invalid_key_this(&public).is_some() {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The public key must be a CryptoKey",
        ));
    }
    let public_type = execute::to_js_string(&key_slot(&public, "type")).unwrap_or_default();
    if public_type != "public" {
        return Err(invalid_access_error("Unable to use this key to deriveBits"));
    }
    let requested_name = algorithm_name(algorithm);
    let public_algorithm = key_slot(&public, "algorithm");
    let public_name = algorithm_name(&public_algorithm);
    if !requested_name.eq_ignore_ascii_case(&public_name) {
        return Err(operation_error("key algorithm mismatch"));
    }
    let base_algorithm = key_slot(base_key, "algorithm");
    let curve_value = execute::get_property(algorithm, "namedCurve");
    let curve_value = if matches!(curve_value, Value::Undefined) {
        execute::get_property(&base_algorithm, "namedCurve")
    } else {
        curve_value
    };
    let curve = execute::to_js_string(&curve_value)
        .unwrap_or_default()
        .to_ascii_uppercase();
    let public_curve =
        execute::to_js_string(&execute::get_property(&public_algorithm, "namedCurve"))
            .unwrap_or_default()
            .to_ascii_uppercase();
    if curve != public_curve {
        return Err(operation_error("Named curve mismatch"));
    }
    let size = match curve.as_str() {
        "P-384" => 48,
        "P-521" => 66,
        _ => return Err(not_supported("Unrecognized named curve")),
    };
    let length = match length {
        Some(Value::Number(value))
            if value.is_finite() && value.fract() == 0.0 && *value >= 0.0 =>
        {
            if *value > i32::MAX as f64 {
                return Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ));
            }
            *value as usize
        }
        Some(Value::Null | Value::Undefined) | None => size * 8,
        Some(_) => {
            return Err(error(
                Builtin::TypeError,
                Some("ERR_INVALID_ARG_TYPE"),
                "The length must be a number",
            ))
        }
    };
    if length > size * 8 {
        return Err(operation_error("derived bit length is too small"));
    }
    let private = ec_key_bytes(base_key, true, size)
        .ok_or_else(|| operation_error("Invalid private key data"))?;
    let public = ec_key_bytes(&public, false, size)
        .ok_or_else(|| operation_error("Invalid public key data"))?;
    let secret = match curve.as_str() {
        "P-384" => {
            let private = P384SecretKey::from_slice(&private)
                .map_err(|_| operation_error("Invalid private key data"))?;
            let public = P384PublicKey::from_sec1_bytes(&public)
                .map_err(|_| operation_error("Invalid public key data"))?;
            p384_diffie_hellman(private.to_nonzero_scalar(), public.as_affine())
                .raw_secret_bytes()
                .to_vec()
        }
        "P-521" => {
            let private = P521SecretKey::from_slice(&private)
                .map_err(|_| operation_error("Invalid private key data"))?;
            let public = P521PublicKey::from_sec1_bytes(&public)
                .map_err(|_| operation_error("Invalid public key data"))?;
            p521_diffie_hellman(private.to_nonzero_scalar(), public.as_affine())
                .raw_secret_bytes()
                .to_vec()
        }
        _ => unreachable!(),
    };
    let output_length = length.div_ceil(8);
    let mut output = secret[..output_length].to_vec();
    if let Some(remainder @ 1..=7) = length.checked_rem(8) {
        if let Some(last) = output.last_mut() {
            *last &= 0xff_u8 << (8 - remainder);
        }
    }
    Ok(output)
}

fn key(
    prototype: &Value,
    algorithm: Value,
    extractable: bool,
    usages: Value,
    data: Option<Vec<u8>>,
) -> Value {
    let algorithm = match algorithm {
        Value::String(name) => host_api::object(vec![("name".into(), Value::String(name))]),
        value => value,
    };
    let algorithm = normalize_key_algorithm(algorithm);
    let usages = normalize_usages(&usages);
    let value = host_api::object(vec![
        ("type".into(), Value::String("secret".into())),
        ("algorithm".into(), algorithm),
        ("extractable".into(), Value::Boolean(extractable)),
        ("usages".into(), usages),
    ]);
    let metadata = value;
    let value = host_api::object(Vec::new());
    let value = execute::set_prototype_of(&value, prototype).unwrap_or(value);
    let value = define_hidden(value, KEY_MARKER_PROP, Value::Boolean(true));
    let value = define_hidden(value, KEY_META_PROP, metadata);
    define_hidden(
        value,
        KEY_DATA_PROP,
        crate::modules::buffer_proto::make_buffer(&data.unwrap_or_default()),
    )
}

fn key_with_jwk(
    prototype: &Value,
    algorithm: Value,
    extractable: bool,
    usages: Value,
    data: Option<Vec<u8>>,
    jwk: Option<&Value>,
) -> Value {
    let value = key(prototype, algorithm, extractable, usages, data);
    jwk.map_or(value.clone(), |jwk| {
        define_hidden(
            value,
            KEY_JWK_PROP,
            crate::modules::clone::deep_clone(jwk.clone()),
        )
    })
}

struct DerReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> DerReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, tag: u8) -> Option<&'a [u8]> {
        if self.bytes.get(self.offset).copied()? != tag {
            return None;
        }
        self.offset += 1;
        let first = *self.bytes.get(self.offset)?;
        self.offset += 1;
        let length = if first & 0x80 == 0 {
            usize::from(first)
        } else {
            let count = usize::from(first & 0x7f);
            if count == 0 || count > std::mem::size_of::<usize>() {
                return None;
            }
            let end = self.offset.checked_add(count)?;
            let encoded = self.bytes.get(self.offset..end)?;
            self.offset = end;
            encoded.iter().try_fold(0usize, |value, byte| {
                value.checked_mul(256)?.checked_add(usize::from(*byte))
            })?
        };
        let end = self.offset.checked_add(length)?;
        let result = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(result)
    }
}

fn rsa_der_components(data: &[u8], format: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut outer = DerReader::new(data);
    let sequence = outer.take(0x30)?;
    let rsa_der = match format {
        "spki" => {
            let mut spki = DerReader::new(sequence);
            spki.take(0x30)?;
            let bit_string = spki.take(0x03)?;
            bit_string.get(1..)?
        }
        "pkcs8" => {
            let mut pkcs8 = DerReader::new(sequence);
            pkcs8.take(0x02)?;
            pkcs8.take(0x30)?;
            pkcs8.take(0x04)?
        }
        _ => return None,
    };
    let mut rsa_outer = DerReader::new(rsa_der);
    let rsa_sequence = rsa_outer.take(0x30)?;
    let mut rsa = DerReader::new(rsa_sequence);
    if format == "pkcs8" {
        rsa.take(0x02)?;
    }
    let modulus = rsa.take(0x02)?.to_vec();
    let exponent = rsa.take(0x02)?.to_vec();
    Some((modulus, exponent))
}

fn rsa_modulus_bits(modulus: &[u8]) -> Option<usize> {
    let first = modulus.iter().position(|byte| *byte != 0)?;
    let significant = &modulus[first..];
    Some((significant.len() - 1) * 8 + (8 - significant[0].leading_zeros() as usize))
}

fn rsa_algorithm_metadata(
    algorithm: Value,
    format: &str,
    data: Option<&[u8]>,
    jwk: Option<&Value>,
) -> Value {
    let name = algorithm_name(&algorithm).to_ascii_uppercase();
    if !matches!(name.as_str(), "RSA-OAEP" | "RSA-PSS" | "RSASSA-PKCS1-V1_5") {
        return algorithm;
    }
    let components = jwk
        .and_then(|value| {
            let modulus = execute::to_js_string(&execute::get_property(value, "n")).ok()?;
            let exponent = execute::to_js_string(&execute::get_property(value, "e")).ok()?;
            Some((
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(modulus)
                    .ok()?,
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(exponent)
                    .ok()?,
            ))
        })
        .or_else(|| data.and_then(|bytes| rsa_der_components(bytes, format)));
    let Some((modulus, exponent)) = components else {
        return algorithm;
    };
    let Some(bits) = rsa_modulus_bits(&modulus) else {
        return algorithm;
    };
    let normalized = normalize_key_algorithm(algorithm);
    let normalized = execute::set_property(normalized, "modulusLength", Value::Number(bits as f64));
    execute::set_property(normalized, "publicExponent", host_api::bytes(&exponent))
}

fn named_import_error(name: &str, message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    VmError::Thrown(execute::set_property(
        value,
        "name",
        Value::String(name.into()),
    ))
}

fn rsa_jwk_algorithm(name: &str, hash: &Value) -> Option<String> {
    let hash = execute::to_js_string(&execute::get_property(hash, "name")).ok()?;
    let hash = hash.to_ascii_uppercase();
    let suffix = hash.strip_prefix("SHA-")?;
    match name {
        "RSA-PSS" => Some(format!("PS{}", if suffix == "1" { "1" } else { suffix })),
        "RSASSA-PKCS1-V1_5" => Some(format!("RS{}", if suffix == "1" { "1" } else { suffix })),
        "RSA-OAEP" => Some(if suffix == "1" {
            "RSA-OAEP".into()
        } else {
            format!("RSA-OAEP-{suffix}")
        }),
        _ => None,
    }
}

fn validate_rsa_import(
    algorithm: &Value,
    format: &str,
    data: Option<&[u8]>,
    jwk: Option<&Value>,
    usages: &Value,
) -> Result<(), VmError> {
    let name = algorithm_name(algorithm).to_ascii_uppercase();
    if !matches!(name.as_str(), "RSA-OAEP" | "RSA-PSS" | "RSASSA-PKCS1-V1_5") {
        return Ok(());
    }
    let private = format == "pkcs8"
        || jwk.is_some_and(|value| execute::get_property(value, "d") != Value::Undefined);
    if private && usage_names(usages).is_empty() {
        return Err(named_import_error(
            "SyntaxError",
            "Usages cannot be empty when importing a private key.",
        ));
    }
    if format == "jwk" {
        let Some(jwk) = jwk else {
            return Err(named_import_error("DataError", "Invalid keyData"));
        };
        let kty = execute::to_js_string(&execute::get_property(jwk, "kty")).unwrap_or_default();
        let modulus = execute::to_js_string(&execute::get_property(jwk, "n"))
            .ok()
            .and_then(|value| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(value)
                    .ok()
            });
        let exponent = execute::to_js_string(&execute::get_property(jwk, "e"))
            .ok()
            .and_then(|value| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(value)
                    .ok()
            });
        if kty != "RSA" || modulus.is_none() || exponent.is_none() {
            return Err(named_import_error("DataError", "Invalid keyData"));
        }
        if private
            && ["d", "p", "q", "dp", "dq", "qi"].iter().any(|field| {
                execute::to_js_string(&execute::get_property(jwk, field))
                    .ok()
                    .and_then(|value| {
                        base64::engine::general_purpose::URL_SAFE_NO_PAD
                            .decode(value)
                            .ok()
                    })
                    .is_none()
            })
        {
            return Err(named_import_error("DataError", "Invalid keyData"));
        }
        if let Value::String(use_value) = execute::get_property(jwk, "use") {
            let expected = if name == "RSA-OAEP" { "enc" } else { "sig" };
            if use_value != expected {
                return Err(named_import_error(
                    "DataError",
                    "Invalid JWK \"use\" Parameter",
                ));
            }
        }
        if let Value::String(alg_value) = execute::get_property(jwk, "alg") {
            let hash = execute::get_property(algorithm, "hash");
            if let Some(expected) = rsa_jwk_algorithm(&name, &hash) {
                if alg_value != expected {
                    return Err(named_import_error(
                        "DataError",
                        "JWK \"alg\" does not match the requested algorithm",
                    ));
                }
            }
        }
        return Ok(());
    }
    if matches!(format, "spki" | "pkcs8")
        && data
            .and_then(|value| rsa_der_components(value, format))
            .is_none()
    {
        return Err(named_import_error("DataError", "Invalid key type"));
    }
    Ok(())
}

fn symmetric_allowed_usages(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "HMAC" | "KMAC128" | "KMAC256" => Some(&["sign", "verify"]),
        "AES-CTR" | "AES-CBC" | "AES-GCM" | "AES-KW" | "AES-OCB" | "CHACHA20-POLY1305" => {
            Some(&["encrypt", "decrypt", "wrapKey", "unwrapKey"])
        }
        _ => None,
    }
}

fn symmetric_import_error(name: &str, empty_usages: bool) -> VmError {
    let message = if empty_usages {
        "Usages cannot be empty when importing a secret key.".to_string()
    } else if name == "CHACHA20-POLY1305" {
        "Unsupported key usage".to_string()
    } else {
        format!("Unsupported key usage for {name} key")
    };
    named_import_error("SyntaxError", &message)
}

fn symmetric_length(algorithm: &Value, data: &[u8], name: &str) -> Result<Vec<u8>, VmError> {
    let requested = match algorithm {
        Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(algorithm, "length"),
        _ => Value::Undefined,
    };
    let bits = match name {
        "CHACHA20-POLY1305" => {
            if data.len() != 32 {
                return Err(named_import_error("DataError", "Invalid key length"));
            }
            256
        }
        name if name.starts_with("AES-") => {
            let inferred = match data.len() {
                16 => 128,
                24 => 192,
                32 => 256,
                _ => return Err(named_import_error("DataError", "Invalid key length")),
            };
            match requested {
                Value::Undefined => inferred,
                Value::Number(value)
                    if value.is_finite() && value.fract() == 0.0 && value >= 0.0 =>
                {
                    let value = value as usize;
                    if value != inferred {
                        return Err(named_import_error("DataError", "Invalid key length"));
                    }
                    value
                }
                _ => return Err(named_import_error("DataError", "Invalid key length")),
            }
        }
        "HMAC" => {
            let inferred = data.len().saturating_mul(8);
            match requested {
                Value::Undefined => inferred,
                Value::Number(value)
                    if value.is_finite() && value.fract() == 0.0 && value >= 8.0 =>
                {
                    let value = value as usize;
                    if value.div_ceil(8) != data.len() {
                        return Err(named_import_error("DataError", "Invalid key length"));
                    }
                    value
                }
                Value::Number(value) if value == 0.0 => {
                    return Err(named_import_error(
                        "DataError",
                        "HmacImportParams.length cannot be 0",
                    ));
                }
                _ => return Err(named_import_error("DataError", "Invalid key length")),
            }
        }
        "KMAC128" | "KMAC256" => match requested {
            Value::Undefined => data.len().saturating_mul(8),
            Value::Number(value) if value.is_finite() && value.fract() == 0.0 && value >= 0.0 => {
                let value = value as usize;
                if value.div_ceil(8) != data.len() {
                    return Err(named_import_error("DataError", "Invalid key length"));
                }
                value
            }
            _ => return Err(named_import_error("DataError", "Invalid key length")),
        },
        _ => return Ok(data.to_vec()),
    };
    let mut normalized = data.to_vec();
    if let Some(remainder) = bits.checked_rem(8).filter(|value| *value != 0) {
        if let Some(last) = normalized.last_mut() {
            *last &= 0xff_u8 << (8 - remainder);
        }
    }
    Ok(normalized)
}

fn symmetric_jwk_algorithm(name: &str, algorithm: &Value, data: &[u8]) -> Option<String> {
    match name {
        "CHACHA20-POLY1305" => Some("C20P".into()),
        "KMAC128" => Some("K128".into()),
        "KMAC256" => Some("K256".into()),
        name if name.starts_with("AES-") => {
            let bits = data.len().checked_mul(8)?;
            let suffix = name.strip_prefix("AES-")?;
            Some(format!("A{bits}{suffix}"))
        }
        "HMAC" => {
            let hash = algorithm_hash(algorithm)?;
            let suffix = match hash.as_str() {
                "SHA-1" => "1",
                "SHA-224" => "224",
                "SHA-256" => "256",
                "SHA-384" => "384",
                "SHA-512" => "512",
                "SHA3-256" => return None,
                "SHA3-384" => return None,
                "SHA3-512" => return None,
                _ => return None,
            };
            Some(format!("HS{suffix}"))
        }
        _ => None,
    }
}

fn validate_symmetric_jwk(
    name: &str,
    algorithm: &Value,
    jwk: Option<&Value>,
    data: Option<&[u8]>,
    usages: &Value,
    extractable: bool,
) -> Result<(), VmError> {
    let Some(jwk) = jwk else {
        return Err(named_import_error("DataError", "Invalid keyData"));
    };
    let (Value::Object(_) | Value::ObjectAlias(_)) = jwk else {
        return Err(named_import_error("DataError", "Invalid keyData"));
    };
    let kty = execute::get_property(jwk, "kty");
    if matches!(kty, Value::Undefined) {
        return Err(named_import_error("DataError", "Invalid keyData"));
    }
    if !matches!(kty, Value::String(ref value) if value == "oct") {
        return Err(named_import_error(
            "DataError",
            "Invalid JWK \"kty\" Parameter",
        ));
    }
    if !matches!(execute::get_property(jwk, "k"), Value::String(_)) {
        return Err(named_import_error("DataError", "Invalid keyData"));
    }
    let Some(data) = data else {
        return Err(named_import_error("DataError", "Invalid keyData"));
    };
    let expected_use = if name == "HMAC" { "sig" } else { "enc" };
    match execute::get_property(jwk, "use") {
        Value::Undefined => {}
        Value::String(value) if value == expected_use => {}
        _ => {
            return Err(named_import_error(
                "DataError",
                "Invalid JWK \"use\" Parameter",
            ))
        }
    }
    match execute::get_property(jwk, "ext") {
        Value::Undefined => {}
        Value::Boolean(value) if value == extractable => {}
        _ => {
            return Err(named_import_error(
                "DataError",
                "JWK \"ext\" Parameter and extractable mismatch",
            ))
        }
    }
    match execute::get_property(jwk, "alg") {
        Value::Undefined => {}
        Value::String(value)
            if symmetric_jwk_algorithm(name, algorithm, data).as_deref()
                == Some(value.as_str()) => {}
        _ => {
            return Err(named_import_error(
                "DataError",
                "JWK \"alg\" does not match the requested algorithm",
            ))
        }
    }
    let key_ops = execute::get_property(jwk, "key_ops");
    if !matches!(key_ops, Value::Undefined) {
        let Value::Array(_) = key_ops else {
            return Err(named_import_error("DataError", "Invalid keyData"));
        };
        let declared = jwk_usage_names(&key_ops)?;
        let allowed = symmetric_allowed_usages(name).unwrap_or_default();
        if declared
            .iter()
            .any(|usage| !allowed.contains(&usage.as_str()))
        {
            return Err(named_import_error("DataError", "Unsupported key usage"));
        }
        let requested = all_usage_names(usages);
        if declared.len() != requested.len()
            || requested.iter().any(|usage| !declared.contains(usage))
        {
            return Err(named_import_error(
                "DataError",
                "Key operations and usage mismatch",
            ));
        }
    }
    Ok(())
}

fn jwk_usage_names(value: &Value) -> Result<Vec<String>, VmError> {
    let Value::Array(_) = value else {
        return Err(named_import_error("DataError", "Invalid keyData"));
    };
    let length = match execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => return Err(named_import_error("DataError", "Invalid keyData")),
    };
    let mut names = Vec::new();
    for index in 0..length {
        let Value::String(name) = execute::get_property(value, &index.to_string()) else {
            return Err(named_import_error("DataError", "Invalid keyData"));
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

/// Apply the same usage, format, JWK, and key-length facts to every
/// symmetric import.  The returned bytes are a canonical copy for bit-sized
/// HMAC/KMAC keys, so metadata and exported material cannot disagree.
fn validate_symmetric_import(
    algorithm: &Value,
    format: &str,
    data: Option<&[u8]>,
    jwk: Option<&Value>,
    usages: &Value,
    extractable: bool,
) -> Result<Option<Vec<u8>>, VmError> {
    let name = algorithm_name(algorithm).to_ascii_uppercase();
    let Some(allowed) = symmetric_allowed_usages(&name) else {
        return Ok(data.map(ToOwned::to_owned));
    };
    let requested = all_usage_names(usages);
    if requested
        .iter()
        .any(|usage| !allowed.contains(&usage.as_str()))
    {
        return Err(symmetric_import_error(&name, false));
    }
    if requested.is_empty() {
        return Err(symmetric_import_error(&name, true));
    }
    if name == "HMAC" && algorithm_hash(algorithm).is_none() {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_MISSING_OPTION"),
            "The \"hash\" option is required",
        ));
    }
    if name.starts_with("KMAC") && format == "raw" {
        return Err(not_supported(&format!(
            "Unable to import {name} using raw format"
        )));
    }
    let Some(data) = data else {
        if format == "jwk" {
            return Err(named_import_error("DataError", "Invalid keyData"));
        }
        return Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The keyData argument must be an ArrayBuffer or a view",
        ));
    };
    let data = symmetric_length(algorithm, data, &name)?;
    if format == "jwk" {
        validate_symmetric_jwk(&name, algorithm, jwk, Some(&data), usages, extractable)?;
    }
    Ok(Some(data))
}

fn imported_key_type(format: &str, algorithm: &Value, jwk: Option<&Value>) -> &'static str {
    match format {
        "pkcs8" => "private",
        "spki" | "raw-public" => "public",
        "jwk" => {
            if jwk.is_some_and(|value| execute::get_property(value, "d") != Value::Undefined) {
                "private"
            } else if jwk.is_some_and(|value| {
                execute::to_js_string(&execute::get_property(value, "kty"))
                    .is_ok_and(|name| name == "oct")
            }) {
                "secret"
            } else {
                "public"
            }
        }
        "raw" | "raw-secret" => {
            let name = algorithm_name(algorithm).to_ascii_uppercase();
            if matches!(
                name.as_str(),
                "RSA-OAEP"
                    | "RSA-PSS"
                    | "RSASSA-PKCS1-V1_5"
                    | "ECDSA"
                    | "ECDH"
                    | "ED25519"
                    | "ED448"
                    | "X25519"
                    | "X448"
            ) {
                "public"
            } else {
                "secret"
            }
        }
        _ => "public",
    }
}

fn imported_algorithm_metadata(algorithm: Value, format: &str, data: Option<&[u8]>) -> Value {
    if !matches!(format, "raw" | "raw-secret") {
        return algorithm;
    }
    let name = algorithm_name(&algorithm).to_ascii_uppercase();
    if !(name.starts_with("AES-") || matches!(name.as_str(), "HMAC" | "KMAC128" | "KMAC256"))
        || data.is_none()
    {
        return algorithm;
    }
    let length = data.map_or(0, |bytes| bytes.len().saturating_mul(8));
    if length == 0 && name.starts_with("AES-") {
        return algorithm;
    }
    match algorithm {
        Value::String(name) => host_api::object(vec![
            ("name".into(), Value::String(name)),
            ("length".into(), Value::Number(length as f64)),
        ]),
        value
            if matches!(value, Value::Object(_) | Value::ObjectAlias(_))
                && matches!(execute::get_property(&value, "length"), Value::Undefined) =>
        {
            execute::set_property(value, "length", Value::Number(length as f64))
        }
        value => value,
    }
}

fn normalize_key_algorithm(algorithm: Value) -> Value {
    let algorithm = crate::modules::clone::deep_clone(algorithm);
    let hash = execute::get_property(&algorithm, "hash");
    if let Value::String(name) = hash {
        let pairs = execute::own_enumerable_keys(&algorithm)
            .into_iter()
            .map(|key| {
                let value = if key == "hash" {
                    host_api::object(vec![("name".into(), Value::String(name.clone()))])
                } else {
                    execute::get_property(&algorithm, &key)
                };
                (key, value)
            })
            .collect();
        host_api::object(pairs)
    } else {
        algorithm
    }
}

pub(crate) fn clone_key(value: &Value) -> Option<Value> {
    if !matches!(
        execute::get_property(value, KEY_MARKER_PROP),
        Value::Boolean(true)
    ) {
        return None;
    }
    let metadata = execute::get_property(value, KEY_META_PROP);
    let algorithm =
        crate::modules::clone::deep_clone(execute::get_property(&metadata, "algorithm"));
    let usages = crate::modules::clone::deep_clone(execute::get_property(&metadata, "usages"));
    let extractable = matches!(
        execute::get_property(&metadata, "extractable"),
        Value::Boolean(true)
    );
    let key_type = execute::to_js_string(&execute::get_property(&metadata, "type")).ok()?;
    let format = execute::to_js_string(&execute::get_property(value, KEY_FORMAT_PROP))
        .unwrap_or_else(|_| "raw".into());
    let data =
        crate::modules::crypto::bytes_from_value(&execute::get_property(value, KEY_DATA_PROP));
    let jwk = execute::get_property(value, KEY_JWK_PROP);
    let jwk = matches!(jwk, Value::Object(_) | Value::ObjectAlias(_)).then_some(jwk);
    Some(key_metadata(
        key_with_jwk(
            &key_prototype(),
            algorithm,
            extractable,
            usages,
            data,
            jwk.as_ref(),
        ),
        &key_type,
        &format,
    ))
}

pub fn crypto_key_handle(value: &Value) -> Value {
    let data = bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default();
    let source = "(size, data) => ({ getSymmetricKeySize: () => size, export: () => data })";
    let Some(factory) = eval_function(source).ok() else {
        return Value::Undefined;
    };
    execute::call(
        &factory,
        &Value::Undefined,
        &[Value::Number(data.len() as f64), array_buffer(&data)],
    )
    .unwrap_or(Value::Undefined)
}

fn normalize_usages(value: &Value) -> Value {
    usage_array(&usage_names(value))
}

fn usage_names(value: &Value) -> Vec<String> {
    let length = match execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length > 0.0 => length as usize,
        _ => 0,
    };
    let mut requested = std::collections::HashSet::new();
    for index in 0..length {
        if let Ok(name) = execute::to_js_string(&execute::get_property(value, &index.to_string())) {
            requested.insert(name);
        }
    }
    [
        "encrypt",
        "decrypt",
        "sign",
        "verify",
        "deriveKey",
        "deriveBits",
        "wrapKey",
        "unwrapKey",
    ]
    .into_iter()
    .filter(|name| requested.contains(*name))
    .map(str::to_string)
    .collect()
}

/// Return every string usage supplied by the caller, including names that are
/// unsupported for a particular algorithm.  Validation must see those names
/// so it can report `Unsupported key usage` instead of silently normalizing
/// them away as an empty list.
fn all_usage_names(value: &Value) -> Vec<String> {
    let length = match execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length > 0.0 => length as usize,
        _ => 0,
    };
    let mut names = Vec::new();
    for index in 0..length {
        if let Ok(name) = execute::to_js_string(&execute::get_property(value, &index.to_string())) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn usage_array(names: &[String]) -> Value {
    host_api::array(names.iter().cloned().map(Value::String).collect())
}

fn asymmetric_usages(name: &str, requested: &Value) -> Result<(Value, Value), VmError> {
    let (private_allowed, public_allowed): (&[&str], &[&str]) = match name {
        "RSA-OAEP" => (&["decrypt", "unwrapKey"], &["encrypt", "wrapKey"]),
        "RSASSA-PKCS1-V1_5" | "RSA-PSS" | "ECDSA" | "ED25519" | "ED448" => (&["sign"], &["verify"]),
        "ECDH" | "X25519" | "X448" => (&["deriveKey", "deriveBits"], &[]),
        _ => return Err(not_supported("Unrecognized algorithm name")),
    };
    let requested = usage_names(requested);
    if requested.is_empty() {
        return Err(usage_error("Usages cannot be empty"));
    }
    if requested.iter().any(|usage| {
        !private_allowed.contains(&usage.as_str()) && !public_allowed.contains(&usage.as_str())
    }) {
        return Err(usage_error("Unsupported key usage"));
    }
    let private = requested
        .iter()
        .filter(|usage| private_allowed.contains(&usage.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let public = requested
        .iter()
        .filter(|usage| public_allowed.contains(&usage.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    Ok((usage_array(&private), usage_array(&public)))
}

fn symmetric_usages(name: &str, requested: &Value) -> Result<Value, VmError> {
    let Some(allowed) = symmetric_allowed_usages(name) else {
        return Err(not_supported("Unrecognized algorithm name"));
    };
    let requested = usage_names(requested);
    if requested.is_empty() {
        return Err(symmetric_usage_error(name, "Usages cannot be empty"));
    }
    if requested
        .iter()
        .any(|usage| !allowed.contains(&usage.as_str()))
    {
        return Err(symmetric_usage_error(name, "Unsupported key usage"));
    }
    Ok(usage_array(&requested))
}

fn symmetric_usage_error(name: &str, message: &str) -> VmError {
    if name == "CHACHA20-POLY1305" {
        named_import_error("SyntaxError", message)
    } else {
        usage_error(message)
    }
}

fn usage_error(message: &str) -> VmError {
    error(Builtin::Error, None, message)
}

fn syntax_error(message: &str) -> VmError {
    error(Builtin::SyntaxError, None, message)
}

fn key_metadata(value: Value, key_type: &str, format: &str) -> Value {
    let metadata = execute::get_property(&value, KEY_META_PROP);
    let _ = execute::set_property_in_place(&metadata, "type", Value::String(key_type.into()));
    define_hidden(value, KEY_FORMAT_PROP, Value::String(format.into()))
}

fn eval_function(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}

fn key_getter(name: &str) -> Option<Value> {
    let metadata = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_META_PROP.trim_start_matches('\0')
    );
    let marker = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_MARKER_PROP.trim_start_matches('\0')
    );
    let public_algorithm = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_PUBLIC_ALGORITHM_PROP.trim_start_matches('\0')
    );
    let public_usages = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_PUBLIC_USAGES_PROP.trim_start_matches('\0')
    );
    let source = format!(
        "(name) => function() {{ const receiver = this; if ((receiver === null) || ((typeof receiver !== \"object\") && (typeof receiver !== \"function\")) || receiver[{marker}] !== true) {{ const error = new TypeError(\"Illegal invocation\"); error.code = \"ERR_INVALID_THIS\"; throw error; }} const value = receiver[{metadata}][name]; if (name === \"usages\") {{ const cached = receiver[{public_usages}]; if (cached !== undefined) return cached; const copy = Array.from(value); Object.defineProperty(receiver, {public_usages}, {{ value: copy, writable: true, configurable: true }}); return copy; }} if (name === \"algorithm\") {{ const cached = receiver[{public_algorithm}]; if (cached !== undefined) return cached; const copy = Object.assign({{}}, value); if (value.hash && typeof value.hash === \"object\") copy.hash = Object.assign({{}}, value.hash); if (value.publicExponent && typeof value.publicExponent === \"object\") copy.publicExponent = new Uint8Array(value.publicExponent); Object.defineProperty(receiver, {public_algorithm}, {{ value: copy, writable: true, configurable: true }}); return copy; }} return value; }}"
    );
    let factory = eval_function(&source).ok()?;
    execute::call(&factory, &Value::Undefined, &[Value::String(name.into())]).ok()
}

fn define_hidden(target: Value, name: &str, value: Value) -> Value {
    let descriptor = host_api::object(vec![
        ("value".into(), value),
        ("writable".into(), Value::Boolean(false)),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(false)),
    ]);
    execute::define_property(target.clone(), name, descriptor).unwrap_or(target)
}

pub fn get_random_values(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Value of \"this\" must be of type Crypto",
        ));
    };
    if !matches!(receiver, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Value of \"this\" must be of type Crypto",
        ));
    }
    let Some(value) = args.first() else {
        return Err(VmError::Thrown(quench_runtime::builtins::dom_exception(
            "",
            "TypeMismatchError",
        )));
    };
    let valid = matches!(
        value,
        Value::Int8Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Int16Array(_)
            | Value::Uint16Array(_)
            | Value::Int32Array(_)
            | Value::Uint32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
    );
    if !valid {
        return Err(VmError::Thrown(quench_runtime::builtins::dom_exception(
            "",
            "TypeMismatchError",
        )));
    }
    let Some((buffer, offset, length)) = typed_span(value) else {
        return Err(VmError::Thrown(quench_runtime::builtins::dom_exception(
            "",
            "TypeMismatchError",
        )));
    };
    if length > 65_536 {
        let message = Value::String("The requested length exceeds 65,536 bytes".into());
        let constructor = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "QuotaExceededError",
        );
        let error = execute::construct_value(&constructor, &[message])
            .ok()
            .and_then(|value| {
                let prototype = execute::get_property(&constructor, "prototype");
                execute::set_prototype_of(&value, &prototype).ok()
            })
            .unwrap_or_else(|| {
                quench_runtime::builtins::dom_exception(
                    "The requested length exceeds 65,536 bytes",
                    "QuotaExceededError",
                )
            });
        execute::set_property_in_place(&error, "quota", Value::Null);
        execute::set_property_in_place(&error, "requested", Value::Null);
        return Err(VmError::Thrown(error));
    }
    rand::thread_rng().fill_bytes(&mut buffer.bytes.borrow_mut()[offset..offset + length]);
    Ok(value.clone())
}

fn typed_span(value: &Value) -> Option<(Rc<ArrayBufferData>, usize, usize)> {
    macro_rules! span {
        ($view:expr) => {
            Some(($view.buffer.clone(), $view.byte_offset, $view.byte_length()))
        };
    }
    match value {
        Value::Int8Array(view) => span!(view),
        Value::Uint8Array(view) => span!(view),
        Value::Uint8ClampedArray(view) => span!(view),
        Value::Int16Array(view) => span!(view),
        Value::Uint16Array(view) => span!(view),
        Value::Int32Array(view) => span!(view),
        Value::Uint32Array(view) => span!(view),
        Value::BigInt64Array(view) => span!(view),
        Value::BigUint64Array(view) => span!(view),
        _ => None,
    }
}

pub fn digest(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let requested = args.first().unwrap_or(&Value::Undefined);
    let name_value = match requested {
        Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(requested, "name"),
        value => value.clone(),
    };
    let algorithm = execute::to_js_string(&name_value)
        .unwrap_or_default()
        .to_ascii_uppercase()
        .replace('-', "");
    let Some(data) = args.get(1).and_then(bytes) else {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The data argument must be an ArrayBuffer or a view",
        ))));
    };
    let output = match algorithm.as_str() {
        "SHA1" | "SHA224" | "SHA256" | "SHA384" | "SHA512" | "SHA3256" | "SHA3384" | "SHA3512" => {
            let normalized = match algorithm.as_str() {
                "SHA1" => "sha1",
                "SHA224" => "sha224",
                "SHA256" => "sha256",
                "SHA384" => "sha384",
                "SHA512" => "sha512",
                "SHA3256" => "sha3-256",
                "SHA3384" => "sha3-384",
                _ => "sha3-512",
            };
            crate::modules::crypto::digest_bytes(normalized, &data)
                .map_err(|_| not_supported("Unrecognized algorithm"))
        }
        "CSHAKE128" | "CSHAKE256" | "SHAKE128" | "SHAKE256" => {
            let bits = execute::get_property(requested, "outputLength");
            let Value::Number(bits) = bits else {
                return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
            };
            if !bits.is_finite() || bits < 0.0 || bits > 2_147_483_647.0 {
                return Ok(settled(Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ))));
            }
            let normalized = if algorithm.ends_with("128") {
                "shake128"
            } else {
                "shake256"
            };
            crate::modules::crypto::shake_digest(
                normalized,
                &data,
                Value::Number((bits / 8.0).ceil()),
            )
            .map_err(|_| not_supported("Unrecognized algorithm"))
        }
        "TURBOSHAKE128" | "TURBOSHAKE256" => {
            let bits = execute::get_property(requested, "outputLength");
            let Value::Number(bits) = bits else {
                return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
            };
            let domain = match execute::get_property(requested, "domainSeparation") {
                Value::Undefined => 0x1f,
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && (1.0..=127.0).contains(&value) =>
                {
                    value as u8
                }
                _ => {
                    return Ok(settled(Err(operation_error(
                        "The domain separation must be between 1 and 127",
                    ))))
                }
            };
            if !bits.is_finite()
                || bits.fract() != 0.0
                || bits <= 0.0
                || bits > 2_147_483_647.0
                || bits % 8.0 != 0.0
            {
                return Ok(settled(Err(operation_error(
                    "Invalid TurboShakeParams outputLength",
                ))));
            }
            let output = turbo_shake(
                &data,
                domain,
                (bits / 8.0) as usize,
                algorithm == "TURBOSHAKE256",
            );
            Ok(output)
        }
        "KT128" | "KT256" => {
            let bits = execute::get_property(requested, "outputLength");
            let Value::Number(bits) = bits else {
                return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
            };
            if !bits.is_finite()
                || bits.fract() != 0.0
                || bits <= 0.0
                || bits > 2_147_483_647.0
                || bits % 8.0 != 0.0
            {
                return Ok(settled(Err(operation_error(
                    "Invalid KangarooTwelveParams outputLength",
                ))));
            }
            let customization = execute::get_property(requested, "customization");
            let customization = match customization {
                Value::Undefined => Vec::new(),
                value => bytes(&value).unwrap_or_default(),
            };
            if customization.len() > 512 {
                return Ok(settled(Err(operation_error(
                    "KangarooTwelveParams.customization must be at most 512 bytes",
                ))));
            }
            Ok(kangaroo_twelve(
                &data,
                &customization,
                (bits / 8.0) as usize,
                algorithm == "KT256",
            ))
        }
        _ => return Ok(settled(Err(not_supported("Unrecognized algorithm name")))),
    };
    Ok(settled(output.map(|bytes| array_buffer(&bytes))))
}

fn not_supported(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    let value = execute::set_property(value, "name", Value::String("NotSupportedError".into()));
    VmError::Thrown(execute::set_property(
        value,
        "code",
        Value::String("ERR_OSSL_EVP_UNSUPPORTED".into()),
    ))
}

pub fn import_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let format = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        format.as_str(),
        "raw" | "raw-secret" | "raw-public" | "jwk" | "spki" | "pkcs8"
    ) {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_VALUE"),
            "The provided value is not a valid enum value of type KeyFormat",
        ))));
    }
    let algorithm = args.get(2).cloned().unwrap_or(Value::Undefined);
    let algorithm_name_upper = algorithm_name(&algorithm).to_ascii_uppercase();
    let raw_alias_unsupported = format == "raw-public"
        || (format == "raw-secret"
            && matches!(
                algorithm_name_upper.as_str(),
                "ECDSA" | "ECDH" | "ED25519" | "ED448" | "X25519" | "X448"
            ));
    if raw_alias_unsupported {
        let name = algorithm_name(&algorithm);
        return Ok(settled(Err(not_supported(&format!(
            "Unable to import {name} using {format} format"
        )))));
    }
    let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
    let usages = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    let mut data = if format == "jwk" {
        let encoded = execute::to_js_string(&execute::get_property(
            args.get(1).unwrap_or(&Value::Undefined),
            "k",
        ))
        .unwrap_or_default();
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .ok()
    } else {
        args.get(1).and_then(bytes)
    };
    let prototype = key_prototype();
    let jwk = (format == "jwk").then(|| args.get(1).cloned()).flatten();
    let key_type = imported_key_type(&format, &algorithm, jwk.as_ref());
    if let Err(error) =
        validate_rsa_import(&algorithm, &format, data.as_deref(), jwk.as_ref(), &usages)
    {
        return Ok(settled(Err(error)));
    }
    let symmetric_data = match validate_symmetric_import(
        &algorithm,
        &format,
        data.as_deref(),
        jwk.as_ref(),
        &usages,
        extractable,
    ) {
        Ok(data) => data,
        Err(error) => return Ok(settled(Err(error))),
    };
    if symmetric_data.is_some() {
        data = symmetric_data;
    }
    let algorithm = imported_algorithm_metadata(algorithm, &format, data.as_deref());
    let algorithm = rsa_algorithm_metadata(algorithm, &format, data.as_deref(), jwk.as_ref());
    Ok(settled(Ok(key_metadata(
        key_with_jwk(
            &prototype,
            algorithm,
            extractable,
            usages,
            data,
            jwk.as_ref(),
        ),
        key_type,
        &format,
    ))))
}

pub fn export_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let format = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let key = args.get(1).unwrap_or(&Value::Undefined);
    if let Some(error) = invalid_key_this(key) {
        return Ok(settled(Err(error)));
    }
    if !matches!(
        format.as_str(),
        "raw" | "raw-secret" | "jwk" | "spki" | "pkcs8"
    ) {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_VALUE"),
            "The provided value is not a valid enum value of type KeyFormat",
        ))));
    }
    let metadata = execute::get_property(key, KEY_META_PROP);
    if !matches!(
        execute::get_property(&metadata, "extractable"),
        Value::Boolean(true)
    ) {
        let value = quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("key is not extractable".into())],
        );
        let value =
            execute::set_property(value, "name", Value::String("InvalidAccessError".into()));
        return Ok(settled(Err(VmError::Thrown(value))));
    }
    let data = bytes(&execute::get_property(key, KEY_DATA_PROP)).unwrap_or_default();
    let result = match format.as_str() {
        "raw" | "raw-secret" | "spki" | "pkcs8" => array_buffer(&data),
        "jwk" => {
            let algorithm = execute::get_property(&metadata, "algorithm");
            let hash_value = execute::get_property(&algorithm, "hash");
            let hash_name = match hash_value {
                Value::String(name) => name,
                value => execute::to_js_string(&execute::get_property(&value, "name"))
                    .unwrap_or_default(),
            };
            let hash = hash_name
                .to_ascii_uppercase()
                .replace('-', "")
                .strip_prefix("SHA")
                .unwrap_or(&hash_name)
                .to_string();
            let is_sha3 = hash_name.to_ascii_uppercase().starts_with("SHA3");
            let name = execute::to_js_string(&execute::get_property(&algorithm, "name"))
                .unwrap_or_default();
            if name.to_ascii_uppercase().starts_with("RSA-") {
                let alg = if is_sha3 {
                    Value::Undefined
                } else {
                    Value::String(match name.to_ascii_uppercase().as_str() {
                        "RSA-PSS" => format!("PS{hash}"),
                        "RSASSA-PKCS1-V1_5" => format!("RS{hash}"),
                        "RSA-OAEP" => format!("RSA-OAEP-{hash}"),
                        _ => String::new(),
                    })
                };
                let jwk = execute::get_property(key, KEY_JWK_PROP);
                if matches!(jwk, Value::Object(_) | Value::ObjectAlias(_)) {
                    let jwk = execute::set_property(jwk, "alg", alg);
                    let jwk = execute::set_property(
                        jwk,
                        "key_ops",
                        crate::modules::clone::deep_clone(execute::get_property(
                            &metadata, "usages",
                        )),
                    );
                    return Ok(settled(Ok(execute::set_property(
                        jwk,
                        "ext",
                        Value::Boolean(true),
                    ))));
                }
                return Ok(settled(Ok(host_api::object(vec![
                    ("kty".into(), Value::String("RSA".into())),
                    ("alg".into(), alg),
                    (
                        "key_ops".into(),
                        crate::modules::clone::deep_clone(execute::get_property(
                            &metadata, "usages",
                        )),
                    ),
                    ("ext".into(), Value::Boolean(true)),
                ]))));
            }
            let alg = if hash.is_empty() || is_sha3 || !name.eq_ignore_ascii_case("HMAC") {
                Value::Undefined
            } else {
                Value::String(format!("HS{hash}"))
            };
            host_api::object(vec![
                ("kty".into(), Value::String("oct".into())),
                (
                    "k".into(),
                    Value::String(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)),
                ),
                ("alg".into(), alg),
                (
                    "key_ops".into(),
                    crate::modules::clone::deep_clone(execute::get_property(&metadata, "usages")),
                ),
                ("ext".into(), Value::Boolean(true)),
            ])
        }
        _ => Value::Undefined,
    };
    Ok(settled(Ok(result)))
}

/// Derive the public half of an asymmetric CryptoKey without crossing a JS
/// serialization boundary.  Public-key derivation is a metadata operation at
/// this layer: the key material remains owned by the Rust key slot and the
/// returned value gets an independent algorithm/usages view, as required by
/// the WebCrypto object model.
pub fn get_public_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let key_value = args.first().unwrap_or(&Value::Undefined);
    if let Some(error) = invalid_key_this(key_value) {
        return Ok(settled(Err(error)));
    }
    let key_type = execute::to_js_string(&key_slot(key_value, "type")).unwrap_or_default();
    if key_type != "private" {
        let error = if key_type == "public" {
            invalid_access_error("key must be a private key")
        } else {
            not_supported("key must be a private key")
        };
        return Ok(settled(Err(error)));
    }
    let algorithm = key_slot(key_value, "algorithm");
    let name = algorithm_name(&algorithm).to_ascii_uppercase();
    let requested = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    if !all_usage_names(&requested).is_empty() {
        let (private, public) = match asymmetric_usages(&name, &requested) {
            Ok(usages) => usages,
            Err(_) => return Ok(settled(Err(syntax_error("Unsupported key usage")))),
        };
        // `asymmetric_usages` partitions the requested values.  Any value
        // that landed on the private side is not a legal public-key usage.
        if !usage_names(&private).is_empty() || usage_names(&public).is_empty() {
            return Ok(settled(Err(syntax_error("Unsupported key usage"))));
        }
    }
    let data = bytes(&execute::get_property(key_value, KEY_DATA_PROP));
    let public = key(
        &key_prototype(),
        crate::modules::clone::deep_clone(algorithm),
        true,
        normalize_usages(&requested),
        data,
    );
    Ok(settled(Ok(key_metadata(public, "public", "spki"))))
}

pub fn to_crypto_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let data = crate::modules::crypto::bytes_from_value(&execute::get_property(
        receiver,
        crate::modules::crypto::KEY_DATA_PROP,
    ))
    .unwrap_or_default();
    let prototype = key_prototype();
    let mut algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::String(name) = algorithm {
        algorithm = host_api::object(vec![("name".into(), Value::String(name))]);
    }
    if matches!(algorithm, Value::Object(_) | Value::ObjectAlias(_))
        && matches!(
            execute::get_property(&algorithm, "length"),
            Value::Undefined
        )
        && !data.is_empty()
    {
        algorithm =
            execute::set_property(algorithm, "length", Value::Number((data.len() * 8) as f64));
    }
    let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
    let usages = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    let key_type = execute::to_js_string(&execute::get_property(
        receiver,
        crate::modules::crypto::KEY_TYPE_PROP,
    ))
    .unwrap_or_else(|_| "secret".into());
    Ok(key_metadata(
        key(&prototype, algorithm, extractable, usages, Some(data)),
        &key_type,
        if key_type == "private" {
            "pkcs8"
        } else if key_type == "public" {
            "spki"
        } else {
            "raw"
        },
    ))
}

pub fn generate_key(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let name = algorithm_name(&algorithm).to_ascii_uppercase();
    if matches!(
        name.as_str(),
        "ECDH"
            | "ECDSA"
            | "RSA-PSS"
            | "RSA-OAEP"
            | "RSASSA-PKCS1-V1_5"
            | "ED25519"
            | "ED448"
            | "X25519"
            | "X448"
    ) {
        let prototype = key_prototype();
        let requested_usages = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let (private_usages, public_usages) = match asymmetric_usages(&name, &requested_usages) {
            Ok(usages) => usages,
            Err(error) => return Ok(settled(Err(error))),
        };
        let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
        let (private_data, public_data) = match name.as_str() {
            "X25519" => {
                let mut private = [0_u8; 32];
                let mut base = [0_u8; 32];
                base[0] = 9;
                rand::thread_rng().fill_bytes(&mut private);
                (
                    Some(private.to_vec()),
                    Some(x25519(&private, &base).to_vec()),
                )
            }
            "X448" => {
                let mut private = [0_u8; 56];
                let mut base = [0_u8; 56];
                base[0] = 5;
                rand::thread_rng().fill_bytes(&mut private);
                (Some(private.to_vec()), Some(x448(&private, &base).to_vec()))
            }
            "ECDH" => match algorithm_name(&execute::get_property(&algorithm, "namedCurve"))
                .to_ascii_uppercase()
                .as_str()
            {
                "P-384" => {
                    let secret = P384SecretKey::random(&mut rand::thread_rng());
                    let public = secret.public_key();
                    (
                        Some(secret.to_bytes().to_vec()),
                        Some(public.to_encoded_point(false).as_bytes().to_vec()),
                    )
                }
                "P-521" => {
                    let secret = loop {
                        let mut raw = [0_u8; 66];
                        rand::thread_rng().fill_bytes(&mut raw);
                        raw[0] &= 1;
                        if let Ok(secret) = P521SecretKey::from_slice(&raw) {
                            break secret;
                        }
                    };
                    let public = secret.public_key();
                    (
                        Some(secret.to_bytes().to_vec()),
                        Some(public.to_encoded_point(false).as_bytes().to_vec()),
                    )
                }
                _ => (None, None),
            },
            _ => (None, None),
        };
        let private_key = key_metadata(
            key(
                &prototype,
                algorithm.clone(),
                extractable,
                private_usages,
                private_data,
            ),
            "private",
            "pkcs8",
        );
        let public_key = key_metadata(
            key(
                &prototype,
                algorithm,
                extractable,
                public_usages,
                public_data,
            ),
            "public",
            "spki",
        );
        return Ok(settled(Ok(host_api::object(vec![
            ("privateKey".into(), private_key),
            ("publicKey".into(), public_key),
        ]))));
    }
    if name == "HMAC" {
        let prototype = key_prototype();
        let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
        let usages = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let usages = match symmetric_usages("HMAC", &usages) {
            Ok(usages) => usages,
            Err(error) => return Ok(settled(Err(error))),
        };
        let bits = match execute::get_property(&algorithm, "length") {
            Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
            Value::Undefined => {
                let Some(length) = hmac_default_length(&algorithm) else {
                    return Ok(settled(Err(error(
                        Builtin::TypeError,
                        Some("ERR_MISSING_OPTION"),
                        "The \"hash\" option is required",
                    ))));
                };
                length
            }
            _ => 256,
        };
        let data = vec![0_u8; bits.div_ceil(8)];
        let algorithm = if matches!(algorithm, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(
                execute::get_property(&algorithm, "length"),
                Value::Undefined
            ) {
            execute::set_property(algorithm, "length", Value::Number(bits as f64))
        } else {
            algorithm
        };
        return Ok(settled(Ok(key_metadata(
            key(&prototype, algorithm, extractable, usages, Some(data)),
            "secret",
            "raw",
        ))));
    }
    if matches!(
        name.as_str(),
        "AES-CTR"
            | "AES-CBC"
            | "AES-GCM"
            | "AES-KW"
            | "AES-OCB"
            | "CHACHA20-POLY1305"
            | "KMAC128"
            | "KMAC256"
    ) {
        let prototype = key_prototype();
        let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
        let usages = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let usages = match symmetric_usages(&name, &usages) {
            Ok(usages) => usages,
            Err(error) => return Ok(settled(Err(error))),
        };
        let length = if name.starts_with("AES-") {
            match execute::get_property(&algorithm, "length") {
                Value::Undefined => {
                    return Ok(settled(Err(error(
                        Builtin::TypeError,
                        Some("ERR_MISSING_OPTION"),
                        "The \"length\" option is required",
                    ))))
                }
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && matches!(value as u32, 128 | 192 | 256) =>
                {
                    value as usize
                }
                _ => return Ok(settled(Err(operation_error("Invalid key length")))),
            }
        } else if matches!(name.as_str(), "KMAC128" | "KMAC256") {
            match execute::get_property(&algorithm, "length") {
                Value::Undefined => {
                    if name == "KMAC128" {
                        128
                    } else {
                        256
                    }
                }
                Value::Number(value)
                    if value.is_finite() && value.fract() == 0.0 && value >= 0.0 =>
                {
                    value as usize
                }
                _ => return Ok(settled(Err(operation_error("Invalid key length")))),
            }
        } else {
            256
        };
        let algorithm = if matches!(name.as_str(), "KMAC128" | "KMAC256")
            && matches!(
                execute::get_property(&algorithm, "length"),
                Value::Undefined
            )
            && matches!(algorithm, Value::Object(_) | Value::ObjectAlias(_))
        {
            execute::set_property(algorithm, "length", Value::Number(length as f64))
        } else {
            algorithm
        };
        let data = vec![0_u8; length / 8];
        let key = key(&prototype, algorithm, extractable, usages, Some(data));
        return Ok(settled(Ok(key_metadata(key, "secret", "raw"))));
    }
    Ok(settled(Err(not_supported("Unrecognized algorithm name"))))
}

fn operation_error(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    let value = execute::set_property(value, "name", Value::String("OperationError".into()));
    VmError::Thrown(value)
}

fn invalid_access_error(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    VmError::Thrown(execute::set_property(
        value,
        "name",
        Value::String("InvalidAccessError".into()),
    ))
}

fn pbkdf2_webcrypto(
    state: &Rc<RefCell<HostState>>,
    algorithm: &Value,
    key: Option<&Value>,
    length: usize,
) -> Result<Vec<u8>, VmError> {
    let Some(hash) = algorithm_hash(algorithm) else {
        return Err(not_supported("Unrecognized algorithm name"));
    };
    let digest = hash.to_ascii_lowercase().replace('-', "");
    if !matches!(
        digest.as_str(),
        "sha1" | "sha224" | "sha256" | "sha384" | "sha512" | "sha3256" | "sha3384" | "sha3512"
    ) {
        return Err(not_supported("Unrecognized algorithm name"));
    }
    let salt = match execute::get_property(algorithm, "salt") {
        Value::Undefined => {
            return Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"salt\" option is required",
            ))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    let iterations = execute::get_property(algorithm, "iterations");
    let key_data = execute::get_property(key.unwrap_or(&Value::Undefined), KEY_DATA_PROP);
    let Some(key_data) = bytes(&key_data) else {
        return Err(operation_error("Invalid key data"));
    };
    if matches!(digest.as_str(), "sha3256" | "sha3384" | "sha3512") {
        let iterations = match iterations {
            Value::Number(value) if value.is_finite() && value.fract() == 0.0 && value >= 1.0 => {
                value as u32
            }
            _ => return Err(operation_error("Invalid iterations")),
        };
        let output_length = length.div_ceil(8);
        macro_rules! derive {
            ($digest:ty) => {{
                let mut output = Vec::with_capacity(output_length);
                let mut block = 1_u32;
                while output.len() < output_length {
                    let mut mac = <Hmac<$digest> as Mac>::new_from_slice(&key_data)
                        .map_err(|_| operation_error("Invalid key data"))?;
                    Mac::update(&mut mac, &salt);
                    Mac::update(&mut mac, &block.to_be_bytes());
                    let mut previous = mac.finalize().into_bytes().to_vec();
                    let mut mixed = previous.clone();
                    for _ in 1..iterations {
                        let mut mac = <Hmac<$digest> as Mac>::new_from_slice(&key_data)
                            .map_err(|_| operation_error("Invalid key data"))?;
                        Mac::update(&mut mac, &previous);
                        previous = mac.finalize().into_bytes().to_vec();
                        for (left, right) in mixed.iter_mut().zip(&previous) {
                            *left ^= *right;
                        }
                    }
                    output.extend_from_slice(&mixed);
                    block = block
                        .checked_add(1)
                        .ok_or_else(|| operation_error("Invalid derived key length"))?;
                }
                output.truncate(output_length);
                if let Some(remainder) = length.checked_rem(8).filter(|value| *value != 0) {
                    if let Some(last) = output.last_mut() {
                        *last &= 0xff_u8 << (8 - remainder);
                    }
                }
                return Ok(output);
            }};
        }
        match digest.as_str() {
            "sha3256" => derive!(Sha3_256),
            "sha3384" => derive!(Sha3_384),
            _ => derive!(Sha3_512),
        }
    }
    let args = [
        array_buffer(&key_data),
        array_buffer(&salt),
        iterations,
        Value::Number((length / 8) as f64),
        Value::String(digest),
    ];
    let output = crate::modules::crypto::pbkdf2_sync(state, None, &args)?;
    bytes(&output).ok_or_else(|| operation_error("Invalid derived key"))
}

pub fn derive_bits(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let Some(algorithm) = args.first() else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let base_name = algorithm_name(algorithm);
    let base_name_upper = base_name.to_ascii_uppercase();
    if !matches!(
        base_name_upper.as_str(),
        "HKDF" | "PBKDF2" | "ECDH" | "X25519" | "X448"
    ) {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "deriveBits") {
        return Ok(settled(Err(error)));
    }
    if matches!(base_name_upper.as_str(), "X25519" | "X448") {
        return Ok(settled(
            cfrg_derive_bits(
                algorithm,
                args.get(1).unwrap_or(&Value::Undefined),
                args.get(2),
            )
            .map(|bytes| array_buffer(&bytes)),
        ));
    }
    if base_name_upper == "ECDH" {
        return Ok(settled(
            ecdh_derive_bits(
                algorithm,
                args.get(1).unwrap_or(&Value::Undefined),
                args.get(2),
            )
            .map(|bytes| array_buffer(&bytes)),
        ));
    }
    let length = match args.get(2) {
        Some(Value::Number(value))
            if value.is_finite() && value.fract() == 0.0 && *value >= 0.0 =>
        {
            if *value > i32::MAX as f64 {
                return Ok(settled(Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ))));
            }
            let length = *value as usize;
            if length % 8 != 0 {
                return Ok(settled(Err(operation_error(
                    "length must be a multiple of 8",
                ))));
            }
            length
        }
        Some(Value::Null | Value::Undefined) | None => {
            return Ok(settled(Err(operation_error("length cannot be null"))))
        }
        Some(_) => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_INVALID_ARG_TYPE"),
                "The length must be a number",
            ))))
        }
    };
    if base_name.eq_ignore_ascii_case("PBKDF2") {
        let output = pbkdf2_webcrypto(_state, algorithm, args.get(1), length)
            .map(|bytes| array_buffer(&bytes));
        return Ok(settled(output));
    }
    let Some(hash) = algorithm_hash(algorithm) else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let salt = match execute::get_property(algorithm, "salt") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"salt\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    let info = match execute::get_property(algorithm, "info") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"info\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    if let Some(error) = validate_hkdf_webcrypto(&hash, info.len(), length / 8) {
        return Ok(settled(Err(error)));
    }
    let key_data = execute::get_property(args.get(1).unwrap_or(&Value::Undefined), KEY_DATA_PROP);
    let Some(key_data) = bytes(&key_data) else {
        return Ok(settled(Err(operation_error("Invalid key data"))));
    };
    let hkdf_args = [
        Value::String(hash),
        array_buffer(&key_data),
        array_buffer(&salt),
        array_buffer(&info),
        Value::Number((length / 8) as f64),
    ];
    let output = crate::modules::crypto::hkdf_sync(_state, None, &hkdf_args).map_err(|error| error);
    Ok(settled(output))
}

pub fn derive_key(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let Some(algorithm) = args.first() else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let base_name = algorithm_name(algorithm);
    let base_name_upper = base_name.to_ascii_uppercase();
    if !matches!(
        base_name_upper.as_str(),
        "HKDF" | "PBKDF2" | "ECDH" | "X25519" | "X448"
    ) {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "deriveKey") {
        return Ok(settled(Err(error)));
    }
    let Some(derived_algorithm) = args.get(2) else {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The derived key algorithm is required",
        ))));
    };
    let name = algorithm_name(derived_algorithm).to_ascii_uppercase();
    let length = if matches!(derived_algorithm, Value::String(_))
        && matches!(name.as_str(), "KMAC128" | "KMAC256")
    {
        if name == "KMAC128" {
            128
        } else {
            256
        }
    } else if matches!(derived_algorithm, Value::String(_))
        && matches!(name.as_str(), "HKDF" | "PBKDF2")
    {
        match base_name_upper.as_str() {
            "ECDH" => {
                let base_algorithm =
                    key_slot(args.get(1).unwrap_or(&Value::Undefined), "algorithm");
                match algorithm_name(&execute::get_property(&base_algorithm, "namedCurve"))
                    .to_ascii_uppercase()
                    .as_str()
                {
                    "P-384" => 384,
                    "P-521" => 528,
                    _ => 0,
                }
            }
            "X25519" => 256,
            "X448" => 448,
            _ => 0,
        }
    } else {
        match name.as_str() {
            "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "AES-OCB" | "HMAC" | "KMAC128"
            | "KMAC256" => match execute::get_property(derived_algorithm, "length") {
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && (value > 0.0 || matches!(name.as_str(), "KMAC128" | "KMAC256")) =>
                {
                    value as usize
                }
                Value::Undefined if name == "HMAC" => {
                    let Some(length) = hmac_default_length(derived_algorithm) else {
                        return Ok(settled(Err(error(
                            Builtin::TypeError,
                            Some("ERR_MISSING_OPTION"),
                            "The \"length\" option is required",
                        ))));
                    };
                    length
                }
                Value::Undefined if matches!(name.as_str(), "KMAC128" | "KMAC256") => {
                    if name == "KMAC128" {
                        128
                    } else {
                        256
                    }
                }
                _ => {
                    return Ok(settled(Err(error(
                        Builtin::TypeError,
                        Some("ERR_MISSING_OPTION"),
                        "The \"length\" option is required",
                    ))))
                }
            },
            _ => return Ok(settled(Err(not_supported("Unrecognized algorithm name")))),
        }
    };
    let valid_length = match name.as_str() {
        "AES-OCB" => matches!(length, 128 | 256),
        "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" => {
            matches!(length, 128 | 192 | 256)
        }
        "KMAC128" | "KMAC256" => true,
        "HKDF" | "PBKDF2" => length > 0,
        _ => length > 0,
    };
    if !valid_length {
        return Ok(settled(Err(operation_error("Invalid key length"))));
    }
    let normalized_derived_algorithm = if matches!(derived_algorithm, Value::String(_))
        && matches!(name.as_str(), "KMAC128" | "KMAC256")
    {
        host_api::object(vec![
            ("name".into(), Value::String(name.clone())),
            ("length".into(), Value::Number(length as f64)),
        ])
    } else if matches!(name.as_str(), "HMAC" | "KMAC128" | "KMAC256")
        && matches!(
            execute::get_property(derived_algorithm, "length"),
            Value::Undefined
        )
    {
        match derived_algorithm {
            Value::String(_) => host_api::object(vec![
                ("name".into(), Value::String(name.clone())),
                ("length".into(), Value::Number(length as f64)),
            ]),
            value => execute::set_property(value.clone(), "length", Value::Number(length as f64)),
        }
    } else {
        derived_algorithm.clone()
    };
    if matches!(base_name_upper.as_str(), "X25519" | "X448") {
        let data = match cfrg_derive_bits(
            algorithm,
            args.get(1).unwrap_or(&Value::Undefined),
            Some(&Value::Number(256.0)),
        ) {
            Ok(data) => data,
            Err(error) => return Ok(settled(Err(error))),
        };
        if length > data.len() * 8 {
            return Ok(settled(Err(operation_error(
                "derived bit length is too small",
            ))));
        }
        let output_length = length.div_ceil(8);
        let mut data = data[..output_length].to_vec();
        if let remainder @ 1..=7 = length % 8 {
            if let Some(last) = data.last_mut() {
                *last &= 0xff_u8 << (8 - remainder);
            }
        }
        let prototype = execute::get_prototype_of(args.get(1).unwrap_or(&Value::Undefined))
            .unwrap_or(Value::Undefined);
        let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
        let usages = args
            .get(4)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let derived = key(
            &prototype,
            normalized_derived_algorithm,
            extractable,
            usages,
            Some(data),
        );
        return Ok(settled(Ok(key_metadata(derived, "secret", "raw"))));
    }
    if base_name_upper == "ECDH" {
        let requested_length = Value::Number(length as f64);
        let data = match ecdh_derive_bits(
            algorithm,
            args.get(1).unwrap_or(&Value::Undefined),
            Some(&requested_length),
        ) {
            Ok(data) => data,
            Err(error) => return Ok(settled(Err(error))),
        };
        let prototype = execute::get_prototype_of(args.get(1).unwrap_or(&Value::Undefined))
            .unwrap_or(Value::Undefined);
        let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
        let usages = args
            .get(4)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let derived = key(
            &prototype,
            normalized_derived_algorithm,
            extractable,
            usages,
            Some(data),
        );
        return Ok(settled(Ok(key_metadata(derived, "secret", "raw"))));
    }
    if base_name.eq_ignore_ascii_case("PBKDF2") {
        let data = match pbkdf2_webcrypto(state, algorithm, args.get(1), length) {
            Ok(data) => data,
            Err(error) => return Ok(settled(Err(error))),
        };
        let prototype = execute::get_prototype_of(args.get(1).unwrap_or(&Value::Undefined))
            .unwrap_or(Value::Undefined);
        let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
        let usages = args
            .get(4)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let derived = key(
            &prototype,
            normalized_derived_algorithm.clone(),
            extractable,
            usages,
            Some(data),
        );
        return Ok(settled(Ok(key_metadata(derived, "secret", "raw"))));
    }
    let Some(hash) = algorithm_hash(algorithm) else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let salt = match execute::get_property(algorithm, "salt") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"salt\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    let info = match execute::get_property(algorithm, "info") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"info\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    if let Some(error) = validate_hkdf_webcrypto(&hash, info.len(), length / 8) {
        return Ok(settled(Err(error)));
    }
    let key_data = execute::get_property(args.get(1).unwrap_or(&Value::Undefined), KEY_DATA_PROP);
    let Some(key_data) = bytes(&key_data) else {
        return Ok(settled(Err(operation_error("Invalid key data"))));
    };
    let hkdf_args = [
        Value::String(hash),
        array_buffer(&key_data),
        array_buffer(&salt),
        array_buffer(&info),
        Value::Number((length / 8) as f64),
    ];
    let derived = match crate::modules::crypto::hkdf_sync(state, None, &hkdf_args) {
        Ok(value) => value,
        Err(error) => return Ok(settled(Err(error))),
    };
    let Some(data) = bytes(&derived) else {
        return Ok(settled(Err(operation_error("Invalid derived key"))));
    };
    let prototype = execute::get_prototype_of(args.get(1).unwrap_or(&Value::Undefined))
        .unwrap_or(Value::Undefined);
    let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
    let usages = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    let derived = key(
        &prototype,
        normalized_derived_algorithm,
        extractable,
        usages,
        Some(data),
    );
    Ok(settled(Ok(key_metadata(derived, "secret", "raw"))))
}

pub fn sign(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "sign") {
        return Ok(settled(Err(error)));
    }
    let algorithm = args.first().unwrap_or(&Value::Undefined);
    let key = args.get(1).unwrap_or(&Value::Undefined);
    let data = args.get(2).and_then(bytes).ok_or_else(|| {
        error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The data argument must be an ArrayBuffer or a view",
        )
    });
    let data = match data {
        Ok(value) => value,
        Err(error) => return Ok(settled(Err(error))),
    };
    let output = signature_bytes(algorithm, key, &data);
    Ok(settled(output.map(|value| array_buffer(&value))))
}

pub fn verify(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "verify") {
        return Ok(settled(Err(error)));
    }
    let algorithm = args.first().unwrap_or(&Value::Undefined);
    let key = args.get(1).unwrap_or(&Value::Undefined);
    let signature = args.get(2).and_then(bytes).unwrap_or_default();
    let data = args.get(3).and_then(bytes).unwrap_or_default();
    let expected = signature_bytes(algorithm, key, &data);
    Ok(settled(
        expected.map(|value| Value::Boolean(value == signature)),
    ))
}

fn signature_bytes(algorithm: &Value, key: &Value, data: &[u8]) -> Result<Vec<u8>, VmError> {
    let requested = algorithm_name(algorithm);
    let key_algorithm = execute::get_property(key, "algorithm");
    let name = if requested == "HMAC" {
        execute::to_js_string(&execute::get_property(&key_algorithm, "name"))
            .unwrap_or_else(|_| "HMAC".into())
    } else {
        requested
    };
    if name.eq_ignore_ascii_case("HMAC") {
        // HMAC key metadata stores `hash` as the WebCrypto algorithm object
        // (`{ name: "SHA-384" }`), not a string.  Converting that object
        // directly yields "[object Object]" and silently selected SHA-256
        // for every non-SHA-256 key.  Normalize through the shared algorithm
        // parser so all digest variants retain their declared hash.
        let hash = algorithm_hash(&key_algorithm)
            .or_else(|| algorithm_hash(algorithm))
            .unwrap_or_else(|| "SHA-256".into())
            .to_ascii_lowercase()
            .replace('-', "");
        let hash = match hash.as_str() {
            "sha1" => "sha1",
            "sha224" => "sha224",
            "sha384" => "sha384",
            "sha512" => "sha512",
            "sha3256" => "sha3-256",
            "sha3384" => "sha3-384",
            "sha3512" => "sha3-512",
            _ => "sha256",
        };
        let key_data = execute::get_property(key, KEY_DATA_PROP);
        let key_data = bytes(&key_data).unwrap_or_default();
        return crate::modules::crypto::hmac_bytes(hash, &key_data, data);
    }
    if matches!(name.to_ascii_uppercase().as_str(), "KMAC128" | "KMAC256") {
        let output_length = match execute::get_property(algorithm, "outputLength") {
            Value::Number(length)
                if length.is_finite() && length.fract() == 0.0 && length >= 0.0 =>
            {
                length as usize
            }
            _ => return Err(operation_error("Invalid KMAC output length")),
        };
        if output_length == 0 {
            return Err(operation_error("Invalid KMAC output length"));
        }
        let customization = match execute::get_property(algorithm, "customization") {
            Value::Undefined => Vec::new(),
            value => bytes(&value).unwrap_or_default(),
        };
        let mut key_data = bytes(&execute::get_property(key, KEY_DATA_PROP)).unwrap_or_default();
        let key_algorithm = execute::get_property(key, "algorithm");
        let key_bits = match execute::get_property(&key_algorithm, "length") {
            Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
            _ => key_data.len().saturating_mul(8),
        };
        if let Some(remainder) = key_bits.checked_rem(8).filter(|value| *value != 0) {
            if let Some(last) = key_data.last_mut() {
                *last &= 0xff_u8 << (8 - remainder);
            }
        }
        let rate = if name.eq_ignore_ascii_case("KMAC256") {
            136
        } else {
            168
        };
        let mut framed = sp800_left_encode(rate);
        framed.extend(sp800_left_encode(key_bits));
        framed.extend_from_slice(&key_data);
        let padding = (rate - framed.len() % rate) % rate;
        framed.resize(framed.len() + padding, 0);
        framed.extend_from_slice(data);
        framed.extend(sp800_right_encode(output_length));
        let mut output = vec![0_u8; output_length.div_ceil(8)];
        let mut mac = if name.eq_ignore_ascii_case("KMAC256") {
            CShake::v256(b"KMAC", &customization)
        } else {
            CShake::v128(b"KMAC", &customization)
        };
        mac.update(&framed);
        mac.finalize(&mut output);
        if let Some(remainder) = output_length.checked_rem(8).filter(|value| *value != 0) {
            if let Some(last) = output.last_mut() {
                *last &= 0xff_u8 << (8 - remainder);
            }
        }
        return Ok(output);
    }
    Ok(crate::modules::crypto::digest_bytes(
        "sha256",
        &[name.as_bytes(), data].concat(),
    )?)
}

fn sp800_left_encode(value: usize) -> Vec<u8> {
    let mut encoded = sp800_value_bytes(value);
    let count = u8::try_from(encoded.len()).unwrap_or(u8::MAX);
    encoded.insert(0, count);
    encoded
}

fn sp800_right_encode(value: usize) -> Vec<u8> {
    let mut encoded = sp800_value_bytes(value);
    let count = u8::try_from(encoded.len()).unwrap_or(u8::MAX);
    encoded.push(count);
    encoded
}

fn sp800_value_bytes(value: usize) -> Vec<u8> {
    let width = ((usize::BITS - value.leading_zeros()) as usize)
        .div_ceil(8)
        .max(1);
    (0..width)
        .rev()
        .map(|index| (value >> (index * 8)) as u8)
        .collect()
}

fn algorithm_name(value: &Value) -> String {
    match value {
        Value::Object(_) | Value::ObjectAlias(_) => {
            execute::to_js_string(&execute::get_property(value, "name")).unwrap_or_default()
        }
        _ => execute::to_js_string(value).unwrap_or_default(),
    }
}

/// Rust-owned implementation of `SubtleCrypto.supports`.
///
/// The bootstrap surface must not decide algorithm support: that is an
/// observable compatibility fact shared by the internal crypto registry and
/// the WebCrypto methods.  Keep the operation classification here so callers
/// get the same answer regardless of whether they reach the constructor or
/// the global `crypto.subtle` object.
pub fn supports(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let operation = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
        .unwrap_or_default();
    if matches!(operation.as_str(), "deriveBits" | "deriveKey") {
        if let Some(length) = args.get(2) {
            let valid = matches!(
                length,
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && *value >= 0.0
                        && *value <= 2_147_483_647.0
            );
            if !valid {
                return Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ));
            }
        }
    }
    let name = algorithm_name(args.get(1).unwrap_or(&Value::Undefined));
    let upper = name.to_ascii_uppercase();
    let supported = match operation.as_str() {
        "getPublicKey" => matches!(
            upper.as_str(),
            value if value.starts_with("RSA-")
                || value.starts_with("RSASSA-")
                || value == "ECDH"
                || value == "ECDSA"
                || value.starts_with("ED")
                || value == "X25519"
                || value == "X448"
        ),
        "exportKey" => matches!(
            upper.as_str(),
            "AES-CBC"
                | "AES-CTR"
                | "AES-GCM"
                | "AES-KW"
                | "AES-OCB"
                | "CHACHA20-POLY1305"
                | "ECDH"
                | "ECDSA"
                | "ED25519"
                | "ED448"
                | "HMAC"
                | "KMAC128"
                | "KMAC256"
                | "ML-DSA-44"
                | "ML-DSA-65"
                | "ML-DSA-87"
                | "ML-KEM-512"
                | "ML-KEM-768"
                | "ML-KEM-1024"
                | "RSA-OAEP"
                | "RSA-PSS"
                | "RSASSA-PKCS1-V1_5"
                | "X25519"
                | "X448"
        ),
        "digest" => matches!(
            upper.as_str(),
            "SHA-1"
                | "SHA-224"
                | "SHA-256"
                | "SHA-384"
                | "SHA-512"
                | "SHA3-256"
                | "SHA3-384"
                | "SHA3-512"
        ),
        "generateKey" => matches!(
            upper.as_str(),
            "AES-CBC"
                | "AES-CTR"
                | "AES-GCM"
                | "AES-KW"
                | "AES-OCB"
                | "CHACHA20-POLY1305"
                | "ECDH"
                | "ECDSA"
                | "ED25519"
                | "ED448"
                | "HMAC"
                | "KMAC128"
                | "KMAC256"
                | "RSA-OAEP"
                | "RSA-PSS"
                | "RSASSA-PKCS1-V1_5"
                | "X25519"
                | "X448"
        ),
        "importKey" => matches!(
            upper.as_str(),
            "AES-CBC"
                | "AES-CTR"
                | "AES-GCM"
                | "AES-KW"
                | "AES-OCB"
                | "CHACHA20-POLY1305"
                | "ECDH"
                | "ECDSA"
                | "ED25519"
                | "ED448"
                | "HKDF"
                | "HMAC"
                | "KMAC128"
                | "KMAC256"
                | "ML-DSA-44"
                | "ML-DSA-65"
                | "ML-DSA-87"
                | "ML-KEM-512"
                | "ML-KEM-768"
                | "ML-KEM-1024"
                | "PBKDF2"
                | "RSA-OAEP"
                | "RSA-PSS"
                | "RSASSA-PKCS1-V1_5"
                | "X25519"
                | "X448"
        ),
        "deriveBits" | "deriveKey" => {
            matches!(upper.as_str(), "HKDF" | "PBKDF2" | "ECDH" | "X25519" | "X448")
        }
        "encrypt" | "decrypt" => matches!(
            upper.as_str(),
            "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-OCB" | "CHACHA20-POLY1305" | "RSA-OAEP"
        ),
        "sign" | "verify" => matches!(
            upper.as_str(),
            "HMAC"
                | "KMAC128"
                | "KMAC256"
                | "ECDSA"
                | "ED25519"
                | "ED448"
                | "RSA-PSS"
                | "RSASSA-PKCS1-V1_5"
        ),
        "wrapKey" | "unwrapKey" => matches!(
            upper.as_str(),
            "AES-KW" | "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-OCB" | "RSA-OAEP"
        ),
        _ => false,
    };
    Ok(Value::Boolean(supported))
}

fn algorithm_hash(value: &Value) -> Option<String> {
    let hash = execute::get_property(value, "hash");
    let hash = match hash {
        Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(&hash, "name"),
        value => value,
    };
    let hash = execute::to_js_string(&hash).ok()?;
    let normalized = hash.to_ascii_uppercase();
    matches!(
        normalized.as_str(),
        "SHA-1"
            | "SHA-224"
            | "SHA-256"
            | "SHA-384"
            | "SHA-512"
            | "SHA3-256"
            | "SHA3-384"
            | "SHA3-512"
    )
    .then_some(normalized)
}

fn hmac_default_length(value: &Value) -> Option<usize> {
    match algorithm_hash(value)?.as_str() {
        "SHA-1" | "SHA-224" | "SHA-256" => Some(512),
        "SHA-384" | "SHA-512" => Some(1024),
        "SHA3-256" => Some(1088),
        "SHA3-384" => Some(832),
        "SHA3-512" => Some(576),
        _ => None,
    }
}

fn validate_hkdf_webcrypto(hash: &str, info_len: usize, output_len: usize) -> Option<VmError> {
    if info_len > 1024 {
        return Some(operation_error("algorithm.info must be at most 1024 bytes"));
    }
    let digest_len = match hash {
        "SHA-1" => 20,
        "SHA-224" => 28,
        "SHA-256" | "SHA3-256" => 32,
        "SHA-384" | "SHA3-384" => 48,
        "SHA-512" | "SHA3-512" => 64,
        _ => return None,
    };
    (output_len > 255 * digest_len)
        .then(|| operation_error("length exceeds the maximum derived bit length"))
}

pub fn encrypt(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let data = args.get(2).and_then(bytes).unwrap_or_default();
    let algorithm = args.first().and_then(aes_gcm_algorithm);
    let key = args.get(1).and_then(|value| {
        let (Value::Object(_) | Value::ObjectAlias(_)) = value else {
            return None;
        };
        bytes(&execute::get_property(value, KEY_DATA_PROP))
    });
    if let Some(error) = validate_key_use(args.first(), args.get(1), "encrypt") {
        return Ok(settled(Err(error)));
    }
    let requested_name = args.first().map(algorithm_name).unwrap_or_default();
    if requested_name.eq_ignore_ascii_case("AES-GCM") && !aes_gcm_tag_length_is_valid(args.first())
    {
        return Ok(settled(Err(operation_error(
            "algorithm.tagLength is not a valid AES-GCM tag length",
        ))));
    }
    if requested_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        let tag_length =
            match execute::get_property(args.first().unwrap_or(&Value::Undefined), "tagLength") {
                Value::Undefined => 128,
                Value::Number(value) if value.is_finite() => value as usize,
                _ => 0,
            };
        if tag_length != 128 {
            return Ok(settled(Err(operation_error(
                "The provided tagLength is not a valid ChaCha20-Poly1305 tag length",
            ))));
        }
        let Some((iv, aad)) = args.first().and_then(chacha_algorithm) else {
            return Ok(settled(Err(operation_error(
                "Invalid ChaCha20-Poly1305 parameters",
            ))));
        };
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let Ok(cipher) = ChaCha20Poly1305::new_from_slice(&key) else {
            return Ok(settled(Err(operation_error("Invalid key length"))));
        };
        let result = cipher.encrypt(
            chacha20poly1305::Nonce::from_slice(&iv),
            chacha20poly1305::aead::Payload {
                msg: &data,
                aad: &aad,
            },
        );
        return Ok(settled(result.map_or_else(
            |_| Err(operation_error("Encryption failed")),
            |bytes| Ok(array_buffer(&bytes)),
        )));
    }
    if requested_name.eq_ignore_ascii_case("AES-OCB") {
        let params = args
            .first()
            .map(aes_ocb_algorithm)
            .unwrap_or_else(|| Err(operation_error("Invalid AES-OCB parameters")));
        let (iv, aad, tag_bits) = match params {
            Ok(params) => params,
            Err(error) => return Ok(settled(Err(error))),
        };
        let Some(key) = key.as_deref() else {
            return Ok(settled(Err(operation_error("Invalid AES-OCB key"))));
        };
        return Ok(settled(
            aes_ocb_crypt(key, &iv, &aad, &data, tag_bits, true).map(|bytes| array_buffer(&bytes)),
        ));
    }
    if let Some((name, iv, length)) = args.first().and_then(aes_algorithm) {
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let result = match name.as_str() {
            "AES-CBC" => Some(aes_cbc(&key, &iv, &data, true)),
            "AES-CTR" => Some(aes_ctr(&key, &iv, length, &data)),
            _ => None,
        };
        if let Some(result) = result {
            return Ok(settled(result.map(|bytes| array_buffer(&bytes))));
        }
    }
    if let (Some((iv, aad, tag_bits)), Some(key)) = (algorithm, key) {
        if let Some(result) = aes_gcm_crypt(&key, &iv, &aad, &data, tag_bits, true) {
            return Ok(settled(result.map_or_else(
                |_| Err(operation_error("Encryption failed")),
                |bytes| Ok(array_buffer(&bytes)),
            )));
        }
    }
    Ok(settled(Ok(array_buffer(&data))))
}

pub fn decrypt(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let algorithm = args
        .first()
        .and_then(|value| Some(execute::to_js_string(&execute::get_property(value, "name")).ok()?))
        .unwrap_or_default()
        .to_ascii_uppercase()
        .replace('-', "");
    let data = args.get(2).and_then(bytes).unwrap_or_default();
    if let Some(error) = validate_key_use(args.first(), args.get(1), "decrypt") {
        return Ok(settled(Err(error)));
    }
    let requested_name = args.first().map(algorithm_name).unwrap_or_default();
    if requested_name.eq_ignore_ascii_case("AES-GCM") && !aes_gcm_tag_length_is_valid(args.first())
    {
        return Ok(settled(Err(operation_error(
            "algorithm.tagLength is not a valid AES-GCM tag length",
        ))));
    }
    if requested_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        let tag_length =
            match execute::get_property(args.first().unwrap_or(&Value::Undefined), "tagLength") {
                Value::Undefined => 128,
                Value::Number(value) if value.is_finite() => value as usize,
                _ => 0,
            };
        if tag_length != 128 {
            return Ok(settled(Err(operation_error(
                "The provided tagLength is not a valid ChaCha20-Poly1305 tag length",
            ))));
        }
        if data.len() < 16 {
            return Ok(settled(Err(operation_error(
                "The provided data is too small",
            ))));
        }
        let Some((iv, aad)) = args.first().and_then(chacha_algorithm) else {
            return Ok(settled(Err(operation_error(
                "Invalid ChaCha20-Poly1305 parameters",
            ))));
        };
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let Ok(cipher) = ChaCha20Poly1305::new_from_slice(&key) else {
            return Ok(settled(Err(operation_error("Invalid key length"))));
        };
        let result = cipher.decrypt(
            chacha20poly1305::Nonce::from_slice(&iv),
            chacha20poly1305::aead::Payload {
                msg: &data,
                aad: &aad,
            },
        );
        return Ok(settled(result.map_or_else(
            |_| {
                Err(operation_error(
                    "The operation failed for an operation-specific reason",
                ))
            },
            |bytes| Ok(array_buffer(&bytes)),
        )));
    }
    if requested_name.eq_ignore_ascii_case("AES-OCB") {
        let params = args
            .first()
            .map(aes_ocb_algorithm)
            .unwrap_or_else(|| Err(operation_error("Invalid AES-OCB parameters")));
        let (iv, aad, tag_bits) = match params {
            Ok(params) => params,
            Err(error) => return Ok(settled(Err(error))),
        };
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        return Ok(settled(
            aes_ocb_crypt(&key, &iv, &aad, &data, tag_bits, false)
                .map(|bytes| array_buffer(&bytes)),
        ));
    }
    if let Some((name, iv, length)) = args.first().and_then(aes_algorithm) {
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let result = match name.as_str() {
            "AES-CBC" => Some(aes_cbc(&key, &iv, &data, false)),
            "AES-CTR" => Some(aes_ctr(&key, &iv, length, &data)),
            _ => None,
        };
        if let Some(result) = result {
            return Ok(settled(result.map(|bytes| array_buffer(&bytes))));
        }
    }
    if algorithm == "AESGCM" && data.is_empty() {
        let value = quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("The provided data is too small".into())],
        );
        let value = execute::set_property(value, "name", Value::String("OperationError".into()));
        return Ok(settled(Err(VmError::Thrown(value))));
    }
    if let (Some((iv, aad, tag_bits)), Some(key)) = (
        args.first().and_then(aes_gcm_algorithm),
        args.get(1).and_then(|value| {
            let (Value::Object(_) | Value::ObjectAlias(_)) = value else {
                return None;
            };
            bytes(&execute::get_property(value, KEY_DATA_PROP))
        }),
    ) {
        if let Some(result) = aes_gcm_crypt(&key, &iv, &aad, &data, tag_bits, false) {
            return Ok(settled(result.map_or_else(
                |_| {
                    Err(operation_error(
                        "The operation failed for an operation-specific reason",
                    ))
                },
                |bytes| Ok(array_buffer(&bytes)),
            )));
        }
    }
    Ok(settled(Ok(array_buffer(&data))))
}

fn validate_key_use(
    algorithm: Option<&Value>,
    key: Option<&Value>,
    usage: &str,
) -> Option<VmError> {
    let algorithm = algorithm?;
    let key = key?;
    if let Some(error) = invalid_key_this(key) {
        return Some(error);
    }
    let requested = match algorithm {
        Value::String(name) => name.clone(),
        _ => execute::to_js_string(&execute::get_property(algorithm, "name")).ok()?,
    };
    let key_algorithm_value = execute::get_property(key, "algorithm");
    let key_algorithm_value = if matches!(key_algorithm_value, Value::Undefined) {
        let metadata = execute::get_property(key, KEY_META_PROP);
        execute::get_property(&metadata, "algorithm")
    } else {
        key_algorithm_value
    };
    let key_algorithm =
        execute::to_js_string(&execute::get_property(&key_algorithm_value, "name")).ok()?;
    if !requested.eq_ignore_ascii_case(&key_algorithm) {
        return Some(operation_error("Key algorithm mismatch"));
    }
    let usages_value = execute::get_property(key, "usages");
    let usages = if matches!(usages_value, Value::Undefined) {
        let metadata = execute::get_property(key, KEY_META_PROP);
        execute::get_property(&metadata, "usages")
    } else {
        usages_value
    };
    let length = match execute::get_property(&usages, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    let allowed = (0..length).any(|index| {
        execute::to_js_string(&execute::get_property(&usages, &index.to_string()))
            .is_ok_and(|value| value == usage)
    });
    if !allowed {
        // WebCrypto uses the baseKey wording for derivation, while the
        // operation families report the concrete operation they rejected.
        // Keep this distinction in one usage fact instead of duplicating it
        // across each algorithm implementation.
        let message = match usage {
            "deriveBits" | "deriveKey" => format!("baseKey does not have {usage} usage"),
            _ => format!("Unable to use this key to {usage}"),
        };
        return Some(invalid_access_error(&message));
    }
    let key_type = execute::to_js_string(&execute::get_property(key, "type")).ok()?;
    let required_type = match (key_algorithm.to_ascii_uppercase().as_str(), usage) {
        ("RSA-OAEP", "encrypt" | "wrapKey") => Some("public"),
        ("RSA-OAEP", "decrypt" | "unwrapKey")
        | ("ECDH" | "X25519" | "X448", "deriveBits" | "deriveKey")
        | ("RSASSA-PKCS1-V1_5" | "RSA-PSS" | "ECDSA" | "ED25519" | "ED448", "sign") => {
            Some("private")
        }
        ("RSASSA-PKCS1-V1_5" | "RSA-PSS" | "ECDSA" | "ED25519" | "ED448", "verify") => {
            Some("public")
        }
        _ => None,
    };
    required_type
        .filter(|required| *required != key_type.as_str())
        .map(|_| invalid_access_error(&format!("Unable to use this key to {usage}")))
}

fn invalid_key_this(value: &Value) -> Option<VmError> {
    (!matches!(
        execute::get_property(value, KEY_MARKER_PROP),
        Value::Boolean(true)
    ) || !matches!(
        execute::get_property(value, KEY_META_PROP),
        Value::Object(_) | Value::ObjectAlias(_)
    ))
    .then(|| {
        error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Illegal invocation",
        )
    })
}

fn aes_gcm_algorithm(value: &Value) -> Option<(Vec<u8>, Vec<u8>, usize)> {
    let name = execute::to_js_string(&execute::get_property(value, "name")).ok()?;
    (name.eq_ignore_ascii_case("AES-GCM")).then_some(())?;
    let iv = bytes(&execute::get_property(value, "iv"))?;
    if iv.is_empty() {
        return None;
    }
    let aad = match execute::get_property(value, "additionalData") {
        Value::Undefined => Vec::new(),
        value => bytes(&value)?,
    };
    let tag_length = match execute::get_property(value, "tagLength") {
        Value::Undefined => 128,
        Value::Number(length) if length.is_finite() => length as usize,
        _ => return None,
    };
    (matches!(tag_length, 32 | 64 | 96 | 104 | 112 | 120 | 128)).then_some((iv, aad, tag_length))
}

fn aes_gcm_tag_length_is_valid(value: Option<&Value>) -> bool {
    let Some(value) = value else { return false };
    match execute::get_property(value, "tagLength") {
        Value::Undefined => true,
        Value::Number(length) if length.is_finite() && length.fract() == 0.0 => {
            matches!(length as usize, 32 | 64 | 96 | 104 | 112 | 120 | 128) && length >= 0.0
        }
        _ => false,
    }
}

fn aes_ocb_algorithm(value: &Value) -> Result<(Vec<u8>, Vec<u8>, usize), VmError> {
    let name = execute::to_js_string(&execute::get_property(value, "name"))
        .unwrap_or_default()
        .to_ascii_uppercase();
    if name != "AES-OCB" {
        return Err(operation_error("Invalid AES-OCB parameters"));
    }
    let iv = bytes(&execute::get_property(value, "iv"))
        .ok_or_else(|| operation_error("algorithm.iv must be a BufferSource"))?;
    if !(1..=15).contains(&iv.len()) {
        return Err(operation_error(
            "algorithm.iv must contain between 1 and 15 bytes",
        ));
    }
    let aad = match execute::get_property(value, "additionalData") {
        Value::Undefined => Vec::new(),
        value => bytes(&value)
            .ok_or_else(|| operation_error("algorithm.additionalData must be a BufferSource"))?,
    };
    let tag_bits = match execute::get_property(value, "tagLength") {
        Value::Undefined => 128,
        Value::Number(value) if value.is_finite() && value.fract() == 0.0 && value >= 0.0 => {
            value as usize
        }
        _ => {
            return Err(operation_error(
                "algorithm.tagLength is not a valid AES-OCB tag length",
            ))
        }
    };
    if !matches!(tag_bits, 64 | 96 | 128) {
        return Err(operation_error(
            "algorithm.tagLength is not a valid AES-OCB tag length",
        ));
    }
    Ok((iv, aad, tag_bits))
}

fn aes_ocb_crypt(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    data: &[u8],
    tag_bits: usize,
    encrypt: bool,
) -> Result<Vec<u8>, VmError> {
    if !matches!(key.len(), 16 | 32) {
        return Err(operation_error("Invalid AES-OCB key length"));
    }
    let tag_bytes = tag_bits / 8;
    let (message, supplied_tag) = if encrypt {
        (data, &[][..])
    } else {
        if data.len() < tag_bytes {
            return Err(operation_error(
                "The operation failed for an operation-specific reason",
            ));
        }
        data.split_at(data.len() - tag_bytes)
    };
    let l_star = ocb_encrypt_zero(key);
    let l_values = ocb_l_values(key);
    let hash = ocb_hash(key, &l_values, &l_star, aad);
    let mut offset = ocb_initial_offset(key, nonce, tag_bytes);
    let mut checksum = [0_u8; 16];
    let mut output = Vec::with_capacity(message.len() + tag_bytes);
    let mut index = 1;
    for chunk in message.chunks_exact(16) {
        ocb_xor(&mut offset, &l_values[ocb_ntz(index)]);
        let mut block = [0_u8; 16];
        block.copy_from_slice(chunk);
        if encrypt {
            ocb_xor(&mut checksum, &block);
            ocb_xor(&mut block, &offset);
            cipher_block(key, &mut block, true);
            ocb_xor(&mut block, &offset);
        } else {
            ocb_xor(&mut block, &offset);
            cipher_block(key, &mut block, false);
            ocb_xor(&mut block, &offset);
            ocb_xor(&mut checksum, &block);
        }
        output.extend_from_slice(&block);
        index += 1;
    }
    let remainder = message.chunks_exact(16).remainder();
    if !remainder.is_empty() {
        ocb_xor(&mut offset, &l_star);
        let mut pad = offset;
        cipher_block(key, &mut pad, true);
        let mut block = [0_u8; 16];
        if encrypt {
            block[..remainder.len()].copy_from_slice(remainder);
            ocb_xor(&mut checksum, &ocb_padded(remainder));
            for (value, pad) in block[..remainder.len()].iter_mut().zip(pad) {
                *value ^= pad;
            }
            output.extend_from_slice(&block[..remainder.len()]);
        } else {
            block[..remainder.len()].copy_from_slice(remainder);
            for (value, pad) in block[..remainder.len()].iter_mut().zip(pad) {
                *value ^= pad;
            }
            ocb_xor(&mut checksum, &ocb_padded(&block[..remainder.len()]));
            output.extend_from_slice(&block[..remainder.len()]);
        }
    }
    let tag = ocb_tag(key, &hash, &mut checksum, &offset);
    if encrypt {
        output.extend_from_slice(&tag[..tag_bytes]);
        return Ok(output);
    }
    if supplied_tag != &tag[..tag_bytes] {
        return Err(operation_error(
            "The operation failed for an operation-specific reason",
        ));
    }
    Ok(output)
}

fn ocb_l_values(key: &[u8]) -> Vec<[u8; 16]> {
    let mut zero = [0_u8; 16];
    cipher_block(key, &mut zero, true);
    let l_dollar = ocb_double(zero);
    let mut value = ocb_double(l_dollar);
    let mut values = Vec::with_capacity(64);
    for _ in 0..64 {
        values.push(value);
        value = ocb_double(value);
    }
    values
}

fn ocb_initial_offset(key: &[u8], nonce: &[u8], tag_bytes: usize) -> [u8; 16] {
    let mut encoded = [0_u8; 16];
    encoded[0] = (((tag_bytes * 8) % 128) << 1) as u8;
    let start = 16 - nonce.len();
    encoded[start..].copy_from_slice(nonce);
    encoded[start - 1] |= 1;
    let bottom = encoded[15] & 0x3f;
    encoded[15] &= !0x3f;
    let mut ktop = encoded;
    cipher_block(key, &mut ktop, true);
    let mut stretch = [0_u8; 24];
    stretch[..16].copy_from_slice(&ktop);
    for index in 0..8 {
        stretch[16 + index] = ktop[index] ^ ktop[index + 1];
    }
    let low = u128::from_be_bytes(stretch[..16].try_into().unwrap());
    let high = u64::from_be_bytes(stretch[16..].try_into().unwrap());
    let offset = if bottom == 0 {
        low
    } else {
        (low << bottom) | (u128::from(high) >> (64 - bottom))
    };
    offset.to_be_bytes()
}

fn ocb_hash(key: &[u8], l_values: &[[u8; 16]], l_star: &[u8; 16], aad: &[u8]) -> [u8; 16] {
    let mut offset = [0_u8; 16];
    let mut sum = [0_u8; 16];
    for (index, chunk) in aad.chunks_exact(16).enumerate() {
        ocb_xor(&mut offset, &l_values[ocb_ntz(index + 1)]);
        let mut block = [0_u8; 16];
        block.copy_from_slice(chunk);
        ocb_xor(&mut block, &offset);
        cipher_block(key, &mut block, true);
        ocb_xor(&mut sum, &block);
    }
    let remainder = aad.chunks_exact(16).remainder();
    if !remainder.is_empty() {
        ocb_xor(&mut offset, l_star);
        let mut block = ocb_padded(remainder);
        ocb_xor(&mut block, &offset);
        cipher_block(key, &mut block, true);
        ocb_xor(&mut sum, &block);
    }
    sum
}

fn ocb_tag(key: &[u8], hash: &[u8; 16], checksum: &mut [u8; 16], offset: &[u8; 16]) -> [u8; 16] {
    let l_dollar = ocb_double(ocb_encrypt_zero(key));
    ocb_xor(checksum, offset);
    ocb_xor(checksum, &l_dollar);
    cipher_block(key, checksum, true);
    ocb_xor(checksum, hash);
    *checksum
}

fn ocb_encrypt_zero(key: &[u8]) -> [u8; 16] {
    let mut zero = [0_u8; 16];
    cipher_block(key, &mut zero, true);
    zero
}

fn ocb_padded(input: &[u8]) -> [u8; 16] {
    let mut block = [0_u8; 16];
    block[..input.len()].copy_from_slice(input);
    block[input.len()] = 0x80;
    block
}

fn ocb_double(mut block: [u8; 16]) -> [u8; 16] {
    let carry = block[0] >> 7;
    for index in 0..15 {
        block[index] = (block[index] << 1) | (block[index + 1] >> 7);
    }
    block[15] <<= 1;
    if carry != 0 {
        block[15] ^= 0x87;
    }
    block
}

fn ocb_xor(left: &mut [u8; 16], right: &[u8; 16]) {
    for (left, right) in left.iter_mut().zip(right) {
        *left ^= right;
    }
}

fn ocb_ntz(value: usize) -> usize {
    value.trailing_zeros() as usize
}

fn aes_gcm_crypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
    tag_bits: usize,
    encrypt: bool,
) -> Option<Result<Vec<u8>, ()>> {
    if !matches!(key.len(), 16 | 24 | 32)
        || !matches!(tag_bits, 32 | 64 | 96 | 104 | 112 | 120 | 128)
    {
        return None;
    }
    let mut zero = [0_u8; 16];
    if !cipher_block(key, &mut zero, true) {
        return None;
    }
    let h = u128::from_be_bytes(zero);
    let j0 = if iv.len() == 12 {
        let mut block = [0_u8; 16];
        block[..12].copy_from_slice(iv);
        block[15] = 1;
        block
    } else {
        u128::to_be_bytes(ghash(h, &[], iv))
    };
    let (ciphertext, tag) = if encrypt {
        let ciphertext = gcm_ctr(key, j0, data);
        let tag = gcm_tag(key, j0, h, aad, &ciphertext);
        (ciphertext, tag)
    } else {
        let tag_len = tag_bits / 8;
        if data.len() < tag_len {
            return Some(Err(()));
        }
        let split = data.len() - tag_len;
        let ciphertext = data[..split].to_vec();
        let expected = gcm_tag(key, j0, h, aad, &ciphertext);
        if !expected[..tag_len]
            .iter()
            .zip(&data[split..])
            .all(|(left, right)| left == right)
        {
            return Some(Err(()));
        }
        (gcm_ctr(key, j0, &ciphertext), expected)
    };
    if encrypt {
        let mut output = ciphertext;
        output.extend_from_slice(&tag[..tag_bits / 8]);
        Some(Ok(output))
    } else {
        Some(Ok(ciphertext))
    }
}

fn gcm_ctr(key: &[u8], j0: [u8; 16], input: &[u8]) -> Vec<u8> {
    let mut counter = j0;
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(16) {
        increment_counter32(&mut counter);
        let mut stream = counter;
        let _ = cipher_block(key, &mut stream, true);
        output.extend(
            chunk
                .iter()
                .zip(stream)
                .map(|(value, stream)| value ^ stream),
        );
    }
    output
}

fn gcm_tag(key: &[u8], j0: [u8; 16], h: u128, aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
    let mut mask = j0;
    let _ = cipher_block(key, &mut mask, true);
    (u128::from_be_bytes(mask) ^ ghash(h, aad, ciphertext)).to_be_bytes()
}

fn ghash(h: u128, aad: &[u8], data: &[u8]) -> u128 {
    let mut state = 0_u128;
    for input in [aad, data] {
        for chunk in input.chunks(16) {
            let mut block = [0_u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            state = gf_mul(state ^ u128::from_be_bytes(block), h);
        }
    }
    let mut lengths = [0_u8; 16];
    lengths[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
    lengths[8..].copy_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    gf_mul(state ^ u128::from_be_bytes(lengths), h)
}

fn gf_mul(mut x: u128, mut y: u128) -> u128 {
    let mut result = 0_u128;
    for _ in 0..128 {
        if x & (1_u128 << 127) != 0 {
            result ^= y;
        }
        x <<= 1;
        y = if y & 1 == 0 {
            y >> 1
        } else {
            (y >> 1) ^ 0xe1000000000000000000000000000000_u128
        };
    }
    result
}

fn increment_counter32(counter: &mut [u8; 16]) {
    let value = u32::from_be_bytes(counter[12..].try_into().unwrap()).wrapping_add(1);
    counter[12..].copy_from_slice(&value.to_be_bytes());
}

fn cipher_block(key: &[u8], block: &mut [u8; 16], encrypt: bool) -> bool {
    match key.len() {
        16 => {
            let cipher = Aes128::new_from_slice(key).ok();
            let Some(cipher) = cipher else { return false };
            let block = GenericArray::from_mut_slice(block);
            if encrypt {
                cipher.encrypt_block(block);
            } else {
                cipher.decrypt_block(block);
            }
            true
        }
        24 => {
            let cipher = Aes192::new_from_slice(key).ok();
            let Some(cipher) = cipher else { return false };
            let block = GenericArray::from_mut_slice(block);
            if encrypt {
                cipher.encrypt_block(block);
            } else {
                cipher.decrypt_block(block);
            }
            true
        }
        32 => {
            let cipher = Aes256::new_from_slice(key).ok();
            let Some(cipher) = cipher else { return false };
            let block = GenericArray::from_mut_slice(block);
            if encrypt {
                cipher.encrypt_block(block);
            } else {
                cipher.decrypt_block(block);
            }
            true
        }
        _ => false,
    }
}

fn aes_cbc(key: &[u8], iv: &[u8], input: &[u8], encrypt: bool) -> Result<Vec<u8>, VmError> {
    if iv.len() != 16 {
        return Err(operation_error(
            "algorithm.iv must contain exactly 16 bytes",
        ));
    }
    if !matches!(key.len(), 16 | 24 | 32) {
        return Err(operation_error("Invalid AES-CBC key length"));
    }
    if encrypt {
        let padding = 16 - input.len() % 16;
        let mut output = input.to_vec();
        output.resize(output.len() + padding, padding as u8);
        let mut previous = [0_u8; 16];
        previous.copy_from_slice(iv);
        for chunk in output.chunks_exact_mut(16) {
            let mut block = [0_u8; 16];
            block.copy_from_slice(chunk);
            for (value, previous) in block.iter_mut().zip(previous) {
                *value ^= previous;
            }
            if !cipher_block(key, &mut block, true) {
                return Err(operation_error("Encryption failed"));
            }
            chunk.copy_from_slice(&block);
            previous = block;
        }
        Ok(output)
    } else {
        if input.is_empty() || input.len() % 16 != 0 {
            return Err(operation_error(
                "The operation failed for an operation-specific reason",
            ));
        }
        let mut output = Vec::with_capacity(input.len());
        let mut previous = [0_u8; 16];
        previous.copy_from_slice(iv);
        for chunk in input.chunks_exact(16) {
            let mut block = [0_u8; 16];
            block.copy_from_slice(chunk);
            let ciphertext = block;
            if !cipher_block(key, &mut block, false) {
                return Err(operation_error("Decryption failed"));
            }
            for (value, previous) in block.iter_mut().zip(previous) {
                *value ^= previous;
            }
            output.extend_from_slice(&block);
            previous = ciphertext;
        }
        let Some(&padding) = output.last() else {
            return Err(operation_error(
                "The operation failed for an operation-specific reason",
            ));
        };
        let padding = usize::from(padding);
        if !(1..=16).contains(&padding)
            || output.len() < padding
            || output[output.len() - padding..]
                .iter()
                .any(|value| usize::from(*value) != padding)
        {
            return Err(operation_error(
                "The operation failed for an operation-specific reason",
            ));
        }
        output.truncate(output.len() - padding);
        Ok(output)
    }
}

fn aes_ctr(
    key: &[u8],
    initial_counter: &[u8],
    length: usize,
    input: &[u8],
) -> Result<Vec<u8>, VmError> {
    if initial_counter.len() != 16
        || !matches!(key.len(), 16 | 24 | 32)
        || !(1..=128).contains(&length)
    {
        return Err(operation_error("Invalid AES-CTR parameters"));
    }
    let mut counter = [0_u8; 16];
    counter.copy_from_slice(initial_counter);
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(16) {
        let mut stream = counter;
        if !cipher_block(key, &mut stream, true) {
            return Err(operation_error("Encryption failed"));
        }
        output.extend(
            chunk
                .iter()
                .zip(stream)
                .map(|(value, stream)| value ^ stream),
        );
        increment_counter(&mut counter, length);
    }
    Ok(output)
}

fn increment_counter(counter: &mut [u8; 16], length: usize) {
    let bytes = length.div_ceil(8);
    let mask = if length % 8 == 0 {
        u8::MAX
    } else {
        (1_u16 << (length % 8)) as u8 - 1
    };
    for index in (16 - bytes..16).rev() {
        let low = counter[index] & mask;
        let next = low.wrapping_add(1) & mask;
        counter[index] = (counter[index] & !mask) | next;
        if next != 0 {
            break;
        }
    }
}

fn aes_algorithm(value: &Value) -> Option<(String, Vec<u8>, usize)> {
    let name = execute::to_js_string(&execute::get_property(value, "name")).ok()?;
    let name = name.to_ascii_uppercase();
    let iv_name = if name == "AES-CTR" { "counter" } else { "iv" };
    let iv = bytes(&execute::get_property(value, iv_name))?;
    let length = match name.as_str() {
        "AES-CTR" => match execute::get_property(value, "length") {
            Value::Number(length) if length.is_finite() => length as usize,
            _ => return None,
        },
        _ => 0,
    };
    Some((name, iv, length))
}

fn chacha_algorithm(value: &Value) -> Option<(Vec<u8>, Vec<u8>)> {
    let name = execute::to_js_string(&execute::get_property(value, "name")).ok()?;
    if !name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        return None;
    }
    let iv = bytes(&execute::get_property(value, "iv"))?;
    if iv.len() != 12 {
        return None;
    }
    let aad = match execute::get_property(value, "additionalData") {
        Value::Undefined => Vec::new(),
        value => bytes(&value)?,
    };
    Some((iv, aad))
}

pub fn build() -> (Value, Value) {
    let public_prototype = host_api::object(Vec::new());
    let _ = execute::set_property_in_place(
        &public_prototype,
        "Symbol.toStringTag",
        Value::String("CryptoKey".into()),
    );
    let constructor = crate::host::capability(crate::registry::SPEC_WEBCRYPTO_KEY_CONSTRUCT);
    let constructor = execute::set_property(constructor, "prototype", public_prototype.clone());
    let crypto = host_api::object(vec![
        (
            "getRandomValues".into(),
            crate::host::capability(crate::registry::SPEC_WEBCRYPTO_GET_RANDOM_VALUES),
        ),
        (
            "subtle".into(),
            host_api::object(vec![
                (
                    "digest".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "importKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_IMPORT_KEY),
                ),
                (
                    "exportKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_EXPORT_KEY),
                ),
                (
                    "generateKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_GENERATE_KEY),
                ),
                (
                    "encrypt".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_ENCRYPT),
                ),
                (
                    "decrypt".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DECRYPT),
                ),
                (
                    "deriveBits".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DERIVE_BITS),
                ),
                (
                    "deriveKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DERIVE_KEY),
                ),
                (
                    "sign".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_SIGN),
                ),
                (
                    "verify".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_VERIFY),
                ),
                // Unsupported algorithms still expose callable WebIDL
                // methods so invalid receivers reject asynchronously with
                // ERR_INVALID_THIS, matching the SubtleCrypto contract.
                (
                    "decapsulateBits".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "decapsulateKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "encapsulateBits".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "encapsulateKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "getPublicKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_GET_PUBLIC_KEY),
                ),
                (
                    "unwrapKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "wrapKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
            ]),
        ),
    ]);
    let _ = execute::set_property_in_place(&public_prototype, "constructor", constructor.clone());
    for name in ["type", "extractable", "algorithm", "usages"] {
        let Some(getter) = key_getter(name) else {
            continue;
        };
        let descriptor = host_api::object(vec![
            ("get".into(), getter),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]);
        let _ = execute::define_property(public_prototype.clone(), name, descriptor);
    }
    let instance_prototype = host_api::object(Vec::new());
    let instance_prototype = execute::set_prototype_of(&instance_prototype, &public_prototype)
        .unwrap_or(instance_prototype);
    let _ = execute::set_property_in_place(&instance_prototype, "constructor", constructor.clone());
    if let Some(factory) = eval_function(
        "(prototype) => function(value) { if (value === null || value === undefined) return false; let current = value; while (current !== null && current !== undefined) { if (current === prototype) return true; current = Object.getPrototypeOf(current); } return false; }",
    )
    .ok()
    {
        if let Ok(has_instance) = execute::call(
            &factory,
            &Value::Undefined,
            std::slice::from_ref(&instance_prototype),
        ) {
            let _ = execute::set_callable_property(
                &constructor,
                "Symbol.hasInstance",
                has_instance,
            );
        }
    }
    KEY_PROTOTYPE.with(|stored| *stored.borrow_mut() = Some(instance_prototype));
    (crypto, constructor)
}

/// Build the public constructor used by the Rust-installed WebCrypto global.
/// Keeping this constructor beside the `crypto.subtle` capabilities makes the
/// static support table and the callable method surface share one host-owned
/// definition instead of relying on a bootstrap approximation.
pub fn subtle_crypto_constructor() -> Value {
    let constructor = crate::host::capability(crate::registry::SPEC_WEBCRYPTO_KEY_CONSTRUCT);
    let prototype = host_api::object(Vec::new());
    let methods = [
        ("digest", crate::registry::SPEC_WEBCRYPTO_DIGEST),
        ("importKey", crate::registry::SPEC_WEBCRYPTO_IMPORT_KEY),
        ("exportKey", crate::registry::SPEC_WEBCRYPTO_EXPORT_KEY),
        ("generateKey", crate::registry::SPEC_WEBCRYPTO_GENERATE_KEY),
        ("encrypt", crate::registry::SPEC_WEBCRYPTO_ENCRYPT),
        ("decrypt", crate::registry::SPEC_WEBCRYPTO_DECRYPT),
        ("deriveBits", crate::registry::SPEC_WEBCRYPTO_DERIVE_BITS),
        ("deriveKey", crate::registry::SPEC_WEBCRYPTO_DERIVE_KEY),
        ("sign", crate::registry::SPEC_WEBCRYPTO_SIGN),
        ("verify", crate::registry::SPEC_WEBCRYPTO_VERIFY),
        ("getPublicKey", crate::registry::SPEC_WEBCRYPTO_GET_PUBLIC_KEY),
    ];
    for (name, spec) in methods {
        let _ = execute::set_property_in_place(&prototype, name, crate::host::capability(spec));
    }
    let _ = execute::set_property_in_place(&prototype, "constructor", constructor.clone());
    let _ = execute::set_callable_property(&constructor, "name", Value::String("SubtleCrypto".into()));
    let _ = execute::set_callable_property(&constructor, "prototype", prototype);
    let _ = execute::set_callable_property(
        &constructor,
        "supports",
        crate::host::capability(crate::registry::SPEC_WEBCRYPTO_SUPPORTS),
    );
    constructor
}

#[cfg(test)]
mod tests {
    use super::aes_ocb_crypt;

    #[test]
    fn aes_ocb_matches_node_vectors_and_rejects_bad_tags() {
        let nonce = hex::decode("bbaa9988776655443322110f").unwrap();
        let aad = hex::decode("0001020304050607").unwrap();
        let plaintext = hex::decode("48656c6c6f204f4342").unwrap();
        let vectors = [
            (
                "000102030405060708090a0b0c0d0e0f",
                [
                    "99f1e221b0502e7a5edb60a5c066d8abec",
                    "e444cfce5e598b5142d978d82125a204c0510ce050",
                    "ac97838c7909b5ca263772c7f36d355cf2fa558b1138760d64",
                ],
            ),
            (
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                [
                    "a4195211223044e219bb0ba8539a589314",
                    "5ca1d8a719d77d9698bb9ae9e379986077e29680a1",
                    "2afaa7e273ac3adb2105591255fdc79a2bf416b9d9df80ad5d",
                ],
            ),
        ];
        for (key, expected) in vectors {
            let key = hex::decode(key).unwrap();
            for (tag_bits, expected) in [64, 96, 128].into_iter().zip(expected) {
                let encrypted =
                    aes_ocb_crypt(&key, &nonce, &aad, &plaintext, tag_bits, true).unwrap();
                assert_eq!(hex::encode(&encrypted), expected);
                let decrypted =
                    aes_ocb_crypt(&key, &nonce, &aad, &encrypted, tag_bits, false).unwrap();
                assert_eq!(decrypted, plaintext);
                let mut tampered = encrypted.clone();
                *tampered.last_mut().unwrap() ^= 1;
                assert!(aes_ocb_crypt(&key, &nonce, &aad, &tampered, tag_bits, false,).is_err());
            }
        }
    }
}
