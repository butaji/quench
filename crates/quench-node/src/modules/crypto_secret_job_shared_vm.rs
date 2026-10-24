//! Draft native SecretKeyGenJob. Uses the configured SDK's key policy and classes.
use super::crypto_secret_material::Material;
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
use std::sync::mpsc::{self, Receiver, TryRecvError};
type Context<'a> = NativeContext<'a, NodeHost>;
#[cfg(quench_openssl_provider)]
#[path = "crypto_kem_request.rs"]
pub(crate) mod kem_request;
#[cfg(quench_openssl_provider)]
#[path = "crypto_kmac_request.rs"]
pub(crate) mod kmac_request;
const ASYNC: u32 = 0;
const SYNC: u32 = 1;
const WEBCRYPTO: u32 = 2;
const BITS_PER_BYTE: u32 = u8::BITS;
const CACHE: &str = "\0crypto:secret-job";
#[derive(Default)]
pub(crate) struct State {
    running: Vec<Running>,
    resources: Vec<Resource>,
    #[cfg(quench_openssl_provider)]
    configured: Vec<Configured>,
    #[cfg(quench_openssl_provider)]
    pub(crate) digests: super::crypto_digest_cache::Registry,
}
struct Resource {
    identity: HostWeakRoot,
    id: u64,
}
#[cfg(quench_openssl_provider)]
struct Configured {
    identity: HostWeakRoot,
    request: NativeRequest,
}
#[cfg(quench_openssl_provider)]
struct HashRequest {
    digest: std::sync::Arc<super::crypto_digest::Digest>,
    input: Option<Vec<u8>>,
    output_length: Option<usize>,
}
#[cfg(quench_openssl_provider)]
struct HmacRequest {
    digest: std::sync::Arc<super::crypto_digest::Digest>,
    key: std::sync::Arc<Material>,
    data: Vec<u8>,
    signature: Option<Vec<u8>>,
}
#[cfg(quench_openssl_provider)]
struct RsaCipherRequest {
    key: std::sync::Arc<super::crypto_asymmetric::OwnedKey>,
    encrypt: bool,
    data: Vec<u8>,
    digest: std::sync::Arc<super::crypto_digest::Digest>,
    label: Vec<u8>,
}
#[cfg(quench_openssl_provider)]
struct DhBitsRequest {
    private_key: std::sync::Arc<super::crypto_asymmetric::OwnedKey>,
    public_key: std::sync::Arc<super::crypto_asymmetric::OwnedKey>,
}
#[cfg(quench_openssl_provider)]
enum NativeRequest {
    Random(usize),
    Aes(AesRequest),
    Argon2(Argon2Request),
    Pbkdf2(Pbkdf2Request),
    Sign(super::crypto_signature_request::Request),
    Hmac(HmacRequest),
    RsaCipher(RsaCipherRequest),
    DhBits(DhBitsRequest),
    Kmac(kmac_request::Request),
    Kem(kem_request::Request),
    Hash(HashRequest),
    Hkdf(HkdfRequest),
    KeyPair(super::crypto_keypair_request::Request),
}
#[cfg(quench_openssl_provider)]
struct AesRequest {
    encrypt: bool,
    key: std::sync::Arc<Material>,
    data: Vec<u8>,
    variant: u32,
    iv: Vec<u8>,
    tag_length: usize,
    additional_data: Vec<u8>,
}
#[cfg(quench_openssl_provider)]
struct HkdfRequest {
    digest: std::sync::Arc<super::crypto_digest::Digest>,
    key: Option<std::sync::Arc<Material>>,
    salt: Option<Vec<u8>>,
    info: Option<Vec<u8>>,
    length: usize,
}
#[cfg(quench_openssl_provider)]
struct Pbkdf2Request {
    digest: std::sync::Arc<super::crypto_digest::Digest>,
    password: Option<std::sync::Arc<Material>>,
    salt: Option<Vec<u8>>,
    iterations: u32,
    length: usize,
}
#[cfg(quench_openssl_provider)]
struct Argon2Request {
    algorithm: u32,
    message: std::sync::Arc<Material>,
    nonce: Vec<u8>,
    parallelism: u32,
    tag_length: usize,
    memory: u32,
    passes: u32,
    secret: Vec<u8>,
    associated_data: Vec<u8>,
}
enum Output {
    #[cfg(quench_openssl_provider)]
    Random(Material),
    #[cfg(quench_openssl_provider)]
    Signature(super::crypto_signature_request::Output),
    Key(Material),
    #[cfg(quench_openssl_provider)]
    Digest(Vec<u8>),
    #[cfg(quench_openssl_provider)]
    Cipher(Vec<u8>),
    #[cfg(quench_openssl_provider)]
    Derived(Material),
    #[cfg(quench_openssl_provider)]
    KeyPair(super::crypto_keypair_request::Pair),
    #[cfg(quench_openssl_provider)]
    Kem(kem_request::Result),
}
enum Failure {
    #[cfg(quench_openssl_provider)]
    Pbkdf2(openssl::error::ErrorStack),
    #[cfg(quench_openssl_provider)]
    Argon2(openssl::error::ErrorStack),
    #[cfg(quench_openssl_provider)]
    Sign(super::crypto_asymmetric::KeyError),
    #[cfg(quench_openssl_provider)]
    RsaCipher(super::crypto_asymmetric::KeyError),
    #[cfg(quench_openssl_provider)]
    DhBits(openssl::error::ErrorStack),
    #[cfg(quench_openssl_provider)]
    Hmac(super::crypto_digest::DigestError),
    Crypto(openssl::error::ErrorStack),
    WorkerDisconnected,
    #[cfg(quench_openssl_provider)]
    Digest(super::crypto_digest::DigestError),
    #[cfg(quench_openssl_provider)]
    Cipher(openssl::error::ErrorStack),
    #[cfg(quench_openssl_provider)]
    Hkdf(openssl::error::ErrorStack),
    #[cfg(quench_openssl_provider)]
    Kmac(openssl::error::ErrorStack),
    #[cfg(quench_openssl_provider)]
    KeyPair(super::crypto_asymmetric::KeyError),
    #[cfg(quench_openssl_provider)]
    Kem {
        decapsulation: bool,
    },
}
type Outcome = Result<Output, Failure>;
enum Delivery {
    Pending(Receiver<Outcome>),
    Ready(Outcome),
}
struct Running {
    identity: HostWeakRoot,
    result: Delivery,
}
fn generate(bits: u32, mode: u32) -> Outcome {
    let bytes = if mode == WEBCRYPTO {
        u64::from(bits).div_ceil(u64::from(BITS_PER_BYTE))
    } else {
        u64::from(bits) / u64::from(BITS_PER_BYTE)
    };
    let mut material = Material::new(vec![0; bytes as usize]);
    openssl::rand::rand_priv_bytes(&mut material).map_err(Failure::Crypto)?;
    if mode == WEBCRYPTO && bits % BITS_PER_BYTE != 0 {
        if let Some(last) = material.last_mut() {
            *last &= u8::MAX << (BITS_PER_BYTE - bits % BITS_PER_BYTE);
        }
    }
    Ok(Output::Key(material))
}
#[cfg(quench_openssl_provider)]
fn aes_cipher(request: AesRequest) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    use openssl::symm::{Cipher, Crypter, Mode};

    if (9..=11).contains(&request.variant) {
        return aes_key_wrap(request);
    }
    let (cipher, kind) = match request.variant {
        0 => (Cipher::aes_128_ctr(), 0),
        1 => (Cipher::aes_192_ctr(), 0),
        2 => (Cipher::aes_256_ctr(), 0),
        3 => (Cipher::aes_128_cbc(), 1),
        4 => (Cipher::aes_192_cbc(), 1),
        5 => (Cipher::aes_256_cbc(), 1),
        6 => (Cipher::aes_128_gcm(), 2),
        7 => (Cipher::aes_192_gcm(), 2),
        8 => (Cipher::aes_256_gcm(), 2),
        12 => (Cipher::aes_128_ocb(), 2),
        13 => (Cipher::aes_192_ocb(), 2),
        14 => (Cipher::aes_256_ocb(), 2),
        15 => (Cipher::chacha20_poly1305(), 2),
        _ => (Cipher::aes_128_cbc(), u8::MAX),
    };
    let mut crypter = Crypter::new(
        cipher,
        if request.encrypt {
            Mode::Encrypt
        } else {
            Mode::Decrypt
        },
        &request.key,
        Some(&request.iv),
    )?;
    if kind == 2 {
        if !request.additional_data.is_empty() {
            crypter.aad_update(&request.additional_data)?;
        }
    }

    let (input, decrypt_tag) = if kind == 2 && !request.encrypt {
        let split = request.data.len().saturating_sub(request.tag_length);
        let (ciphertext, tag) = request.data.split_at(split);
        crypter.set_tag(tag)?;
        (ciphertext, Some(tag))
    } else {
        (request.data.as_slice(), None)
    };
    let mut output = vec![0; input.len() + cipher.block_size()];
    let mut written = crypter.update(input, &mut output)?;
    written += crypter.finalize(&mut output[written..])?;
    output.truncate(written);
    if kind == 2 && request.encrypt {
        let mut tag = vec![0; request.tag_length];
        crypter.get_tag(&mut tag)?;
        output.extend_from_slice(&tag);
    }
    let _ = decrypt_tag;
    Ok(output)
}
#[cfg(quench_openssl_provider)]
fn aes_key_wrap(request: AesRequest) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    use openssl::symm::{Cipher, Crypter, Mode};

    let cipher = match request.variant {
        9 => Cipher::aes_128_ecb(),
        10 => Cipher::aes_192_ecb(),
        _ => Cipher::aes_256_ecb(),
    };
    let transform = |block: &[u8; 16], mode| {
        let mut crypter = Crypter::new(cipher, mode, &request.key, None)?;
        crypter.pad(false);
        let mut output = [0u8; 32];
        let length = crypter.update(block, &mut output)?;
        let length = length + crypter.finalize(&mut output[length..])?;
        let mut result = [0u8; 16];
        result.copy_from_slice(&output[..length]);
        Ok::<_, openssl::error::ErrorStack>(result)
    };
    let input_blocks = request.data.len() / 8;
    let n = if request.encrypt {
        input_blocks
    } else {
        input_blocks.saturating_sub(1)
    };
    if request.data.len() % 8 != 0 || n < 2 {
        return Err(openssl::error::ErrorStack::get());
    }
    let mut blocks = request
        .data
        .chunks_exact(8)
        .map(|chunk| <[u8; 8]>::try_from(chunk).expect("chunk is eight bytes"))
        .collect::<Vec<_>>();
    if request.encrypt {
        let mut a = [0xa6u8; 8];
        let mut r = blocks;
        for j in 0..6u64 {
            for i in 0..n {
                let mut block = [0u8; 16];
                block[..8].copy_from_slice(&a);
                block[8..].copy_from_slice(&r[i]);
                let mut encrypted = transform(&block, Mode::Encrypt)?;
                let t = j * n as u64 + i as u64 + 1;
                for (byte, mask) in encrypted[..8].iter_mut().zip(t.to_be_bytes()) {
                    *byte ^= mask;
                }
                a.copy_from_slice(&encrypted[..8]);
                r[i].copy_from_slice(&encrypted[8..]);
            }
        }
        let mut output = Vec::with_capacity((n + 1) * 8);
        output.extend_from_slice(&a);
        for block in r {
            output.extend_from_slice(&block);
        }
        Ok(output)
    } else {
        let mut a = blocks.remove(0);
        let mut r = blocks;
        for j in (0..6u64).rev() {
            for i in (0..n).rev() {
                let t = j * n as u64 + i as u64 + 1;
                let mut block = [0u8; 16];
                for ((out, left), right) in block[..8].iter_mut().zip(a).zip(t.to_be_bytes()) {
                    *out = left ^ right;
                }
                block[8..].copy_from_slice(&r[i]);
                let decrypted = transform(&block, Mode::Decrypt)?;
                a.copy_from_slice(&decrypted[..8]);
                r[i].copy_from_slice(&decrypted[8..]);
            }
        }
        if a != [0xa6; 8] {
            return Err(openssl::error::ErrorStack::get());
        }
        let mut output = Vec::with_capacity(n * 8);
        for block in r {
            output.extend_from_slice(&block);
        }
        Ok(output)
    }
}
fn get(c: &mut Context<'_>, o: RootId, name: &str) -> Result<RootId, RootedError> {
    let name = c.string_rooted(name);
    c.get_property_rooted(o, name)
}
fn put(c: &mut Context<'_>, o: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let name = c.string_rooted(name);
    c.define_data_property_rooted(o, name, value, true, true, true)?;
    Ok(())
}
fn fail(c: &mut Context<'_>, text: &str) -> RootedError {
    match c.type_error_rooted(text) {
        Ok(e) => c.throw(e),
        Err(e) => e,
    }
}
fn uint(c: &mut Context<'_>, arg: RootId) -> Result<u32, RootedError> {
    let n = c.rooted_value(arg).and_then(|v| v.as_number());
    match n {
        Some(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => Ok(n as u32),
        _ => Err(fail(c, "Expected uint32 crypto job argument")),
    }
}
fn api(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .builtin_cache
        .get(CACHE)
        .copied()
        .ok_or_else(|| RootedError::host("Secret job binding not installed"))
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    let api = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let wm = get(c, global, "WeakMap")?;
        let map = c.construct_rooted(wm, wm, &[])?;
        let proto = get(c, wm, "prototype")?;
        let setter = get(c, proto, "set")?;
        let api = c.null_object_rooted()?;
        put(c, api, "jobs", map)?;
        put(c, api, "set", setter)?;
        for name in ["Promise", "DOMException"] {
            let ctor = get(c, global, name)?;
            put(c, api, name, ctor)?;
        }
        let reflect = get(c, global, "Reflect")?;
        let delete = get(c, reflect, "deleteProperty")?;
        put(c, api, "delete", delete)?;
        Ok(api)
    })?;
    let root = c.retain(api)?;
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(CACHE.into(), root);
    install_job_class(c, binding, "SecretKeyGenJob", "cryptoSecretJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "HashJob", "cryptoHashJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "AESCipherJob", "cryptoAesJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(
        c,
        binding,
        "ChaCha20Poly1305CipherJob",
        "cryptoChaChaJobNew",
    )?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "RSACipherJob", "cryptoRsaCipherJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "DHBitsJob", "cryptoDhBitsJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "HKDFJob", "cryptoHkdfJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "PBKDF2Job", "cryptoPbkdf2JobNew")?;
    #[cfg(quench_openssl_provider)]
    if super::crypto_argon2::available() {
        install_job_class(c, binding, "Argon2Job", "cryptoArgon2JobNew")?;
        for (name, value) in [
            ("kTypeArgon2d", 0),
            ("kTypeArgon2i", 1),
            ("kTypeArgon2id", 2),
        ] {
            let value = c.number(value as f64);
            put(c, binding, name, value)?;
        }
    }
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "SignJob", "cryptoSignJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "HmacJob", "cryptoHmacJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "KmacJob", "cryptoKmacJobNew")?;
    #[cfg(quench_openssl_provider)]
    install_job_class(
        c,
        binding,
        "KEMEncapsulateJob",
        "cryptoKemEncapsulateJobNew",
    )?;
    #[cfg(quench_openssl_provider)]
    install_job_class(
        c,
        binding,
        "KEMDecapsulateJob",
        "cryptoKemDecapsulateJobNew",
    )?;
    #[cfg(quench_openssl_provider)]
    install_job_class(c, binding, "RandomBytesJob", "cryptoRandomJobNew")?;
    #[cfg(quench_openssl_provider)]
    for &(name, value) in super::crypto_signature_request::CONSTANTS {
        let value = c.number(value as f64);
        put(c, binding, name, value)?;
    }
    #[cfg(quench_openssl_provider)]
    for (name, operation) in [
        ("RsaKeyPairGenJob", "cryptoRsaKeyPairJobNew"),
        ("EcKeyPairGenJob", "cryptoEcKeyPairJobNew"),
        ("DsaKeyPairGenJob", "cryptoDsaKeyPairJobNew"),
        ("DhKeyPairGenJob", "cryptoDhKeyPairJobNew"),
        ("NamedKeyPairGenJob", "cryptoNamedKeyPairJobNew"),
    ] {
        install_job_class(c, binding, name, operation)?;
    }
    #[cfg(quench_openssl_provider)]
    for (name, value) in [
        ("kKeyVariantRSA_SSA_PKCS1_v1_5", 0),
        ("kKeyVariantRSA_PSS", 1),
        ("kKeyVariantRSA_OAEP", 2),
        ("OPENSSL_EC_NAMED_CURVE", 1),
        ("OPENSSL_EC_EXPLICIT_CURVE", 0),
    ] {
        let value = c.number(f64::from(value));
        put(c, binding, name, value)?;
    }
    for (name, value) in [
        ("kCryptoJobAsync", ASYNC),
        ("kCryptoJobSync", SYNC),
        ("kCryptoJobWebCrypto", WEBCRYPTO),
        ("kWebCryptoCipherEncrypt", 0),
        ("kWebCryptoCipherDecrypt", 1),
    ] {
        let value = c.number(value as f64);
        put(c, binding, name, value)?;
    }
    #[cfg(quench_openssl_provider)]
    for (name, value) in [
        ("kKeyVariantAES_CTR_128", 0),
        ("kKeyVariantAES_CTR_192", 1),
        ("kKeyVariantAES_CTR_256", 2),
        ("kKeyVariantAES_CBC_128", 3),
        ("kKeyVariantAES_CBC_192", 4),
        ("kKeyVariantAES_CBC_256", 5),
        ("kKeyVariantAES_GCM_128", 6),
        ("kKeyVariantAES_GCM_192", 7),
        ("kKeyVariantAES_GCM_256", 8),
        ("kKeyVariantAES_KW_128", 9),
        ("kKeyVariantAES_KW_192", 10),
        ("kKeyVariantAES_KW_256", 11),
        ("kKeyVariantAES_OCB_128", 12),
        ("kKeyVariantAES_OCB_192", 13),
        ("kKeyVariantAES_OCB_256", 14),
    ] {
        let value = c.number(value as f64);
        put(c, binding, name, value)?;
    }
    Ok(())
}
fn install_job_class(
    c: &mut Context<'_>,
    binding: RootId,
    name: &str,
    operation: &str,
) -> Result<(), RootedError> {
    let constructor = c.host_constructor(crate::host::shared_vm::operation(operation))?;
    let key = c.string_rooted("name");
    let name_value = c.string_rooted(name);
    c.define_data_property_rooted(constructor, key, name_value, false, false, true)?;
    let key = c.string_rooted("length");
    let zero = c.number(0.0);
    c.define_data_property_rooted(constructor, key, zero, false, false, true)?;
    let prototype = get(c, constructor, "prototype")?;
    // V8 gives each job class its own wrapper; the shared native operation is
    // authoritative while method identity remains constructor-specific.
    let run = c.host_function_named(
        crate::host::shared_vm::operation("cryptoSecretJobRun"),
        "run",
    )?;
    put(c, prototype, "run", run)?;
    put(c, binding, name, constructor)
}
fn record(c: &mut Context<'_>, job: RootId) -> Result<RootId, RootedError> {
    let api = api(c)?;
    let map = get(c, api, "jobs")?;
    let record = c.weak_map_get_rooted(map, job)?;
    if !c.is_object_rooted(record)? {
        return Err(fail(c, "Invalid SecretKeyGenJob receiver"));
    }
    Ok(record)
}
pub(crate) fn create(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |i: usize| args.get(i).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    uint(c, arg(1))?;
    let fields = [
        ("mode", arg(0)),
        ("bits", arg(1)),
        ("algorithm", arg(2)),
        ("usages", arg(3)),
        ("extractable", arg(4)),
    ];
    register(
        c,
        job,
        &fields,
        "KEYGENREQUEST",
        #[cfg(quench_openssl_provider)]
        None,
    )
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_random(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode != ASYNC && mode != SYNC {
        return Err(fail(c, "Invalid random bytes job mode"));
    }
    let offset = uint(c, arg(2))? as usize;
    let size = uint(c, arg(3))? as usize;
    // Capture intrinsic backing/byte range before init hooks can change public
    // properties. This rooted view never crosses the worker boundary.
    let (destination, setter) =
        super::crypto_provider_shared_vm::random_destination(c, arg(1), offset, size)?;
    register(
        c,
        job,
        &[
            ("mode", arg(0)),
            ("randomDestination", destination),
            ("randomSetter", setter),
        ],
        "RANDOMBYTESREQUEST",
        Some(NativeRequest::Random(size)),
    )
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_aes(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode != WEBCRYPTO {
        return Err(fail(c, "Invalid AES crypto job mode"));
    }
    let cipher_mode = uint(c, arg(1))?;
    if cipher_mode > 1 {
        return Err(fail(c, "Invalid AES cipher mode"));
    }
    let key = super::crypto_key_material_shared_vm::secret(c, arg(2))?
        .ok_or_else(|| fail(c, "Invalid AES CryptoKey handle"))?;
    let data = c
        .bytes_prefix_rooted(arg(3), usize::MAX)
        .ok_or_else(|| fail(c, "AES input must be a byte source"))?;
    let variant = uint(c, arg(4))?;
    let iv = c
        .bytes_prefix_rooted(arg(5), usize::MAX)
        .unwrap_or_default();
    // AES-CBC jobs reject invalid IV sizes when the job is constructed. Keep
    // this synchronous check here; deferring it to OpenSSL changes the public
    // CryptoJob contract and causes jobPromise() to observe a different path.
    if (3..=5).contains(&variant) && iv.len() != 16 {
        return Err(fail(c, "Invalid initialization vector"));
    }
    let tag_length = c
        .rooted_value(arg(6))
        .and_then(quench_runtime::Value::as_number)
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .map(|length| length as usize)
        .unwrap_or(16);
    let additional_data = c
        .bytes_prefix_rooted(arg(7), usize::MAX)
        .unwrap_or_default();
    let request = AesRequest {
        encrypt: cipher_mode == 0,
        key,
        data,
        variant,
        iv,
        tag_length,
        additional_data,
    };
    register(
        c,
        job,
        &[("mode", arg(0))],
        "AES_CIPHERJOB",
        Some(NativeRequest::Aes(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_chacha(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    if uint(c, arg(0))? != WEBCRYPTO {
        return Err(fail(c, "Invalid ChaCha20-Poly1305 crypto job mode"));
    }
    let cipher_mode = uint(c, arg(1))?;
    if cipher_mode > 1 {
        return Err(fail(c, "Invalid ChaCha20-Poly1305 cipher mode"));
    }
    let key = super::crypto_key_material_shared_vm::secret(c, arg(2))?
        .ok_or_else(|| fail(c, "Invalid ChaCha20-Poly1305 CryptoKey handle"))?;
    let data = c
        .bytes_prefix_rooted(arg(3), usize::MAX)
        .ok_or_else(|| fail(c, "ChaCha20-Poly1305 input must be a byte source"))?;
    let iv = c
        .bytes_prefix_rooted(arg(4), usize::MAX)
        .unwrap_or_default();
    let additional_data = c
        .bytes_prefix_rooted(arg(5), usize::MAX)
        .unwrap_or_default();
    let request = AesRequest {
        encrypt: cipher_mode == 0,
        key,
        data,
        variant: 15,
        iv,
        tag_length: 16,
        additional_data,
    };
    register(
        c,
        job,
        &[("mode", arg(0))],
        "CHACHA20_POLY1305_CIPHERJOB",
        Some(NativeRequest::Aes(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_rsa_cipher(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode != WEBCRYPTO {
        return Err(fail(c, "Invalid RSA crypto job mode"));
    }
    let cipher_mode = uint(c, arg(1))?;
    if cipher_mode > 1 {
        return Err(fail(c, "Invalid RSA cipher mode"));
    }
    let handle = arg(2);
    let key = super::crypto_asymmetric_shared_vm::key(c, handle)?
        .ok_or_else(|| fail(c, "Invalid RSA CryptoKey handle"))?;
    let expected_kind = if cipher_mode == 0 {
        super::crypto_asymmetric::KeyKind::Public
    } else {
        super::crypto_asymmetric::KeyKind::Private
    };
    if key.kind() != expected_kind {
        return Err(fail(c, "Invalid RSA CryptoKey type"));
    }
    let data = c
        .bytes_prefix_rooted(arg(3), usize::MAX)
        .ok_or_else(|| fail(c, "RSA input must be a byte source"))?;
    if uint(c, arg(4))? != 2 {
        return Err(fail(c, "Unsupported RSA cipher key variant"));
    }
    let name = c
        .string_text(arg(5))?
        .ok_or_else(|| fail(c, "RSA digest must be a string"))?;
    let digest = std::sync::Arc::new(
        super::crypto_digest::Digest::from_sdk_name(&name)
            .map_err(|_| invalid_digest(c, &format!("Invalid digest: {name}")))?,
    );
    let label = c
        .bytes_prefix_rooted(arg(6), usize::MAX)
        .unwrap_or_default();
    register(
        c,
        job,
        &[("mode", arg(0))],
        "RSA_CIPHERREQUEST",
        Some(NativeRequest::RsaCipher(RsaCipherRequest {
            key,
            encrypt: cipher_mode == 0,
            data,
            digest,
            label,
        })),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_dh_bits(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode != WEBCRYPTO {
        return Err(fail(c, "Invalid Diffie-Hellman crypto job mode"));
    }
    let public_kind = c.number(1.0);
    let private_kind = c.number(2.0);
    let public_args = [public_kind, arg(1), arg(2), arg(3), arg(4), arg(5)];
    let private_args = [private_kind, arg(6), arg(7), arg(8), arg(9), arg(10)];
    let public_key = super::crypto_asymmetric_shared_vm::import_input(
        c,
        super::crypto_asymmetric::KeyKind::Public,
        &public_args,
    )?;
    let private_key = super::crypto_asymmetric_shared_vm::import_input(
        c,
        super::crypto_asymmetric::KeyKind::Private,
        &private_args,
    )?;
    if !matches!(
        (public_key.key_type(), private_key.key_type()),
        (Some(public), Some(private)) if public == private
    ) {
        return Err(fail(c, "Incompatible key types for Diffie-Hellman"));
    }
    register(
        c,
        job,
        &[("mode", arg(0))],
        "DHBITSREQUEST",
        Some(NativeRequest::DhBits(DhBitsRequest {
            private_key,
            public_key,
        })),
    )
}
fn register(
    c: &mut Context<'_>,
    job: RootId,
    fields: &[(&str, RootId)],
    resource_type: &str,
    #[cfg(quench_openssl_provider)] request: Option<NativeRequest>,
) -> Result<RootId, RootedError> {
    let api = api(c)?;
    let record = c.null_object_rooted()?;
    for (name, value) in fields {
        put(c, record, name, *value)?;
    }
    let no = c.boolean(false);
    put(c, record, "started", no)?;
    let map = get(c, api, "jobs")?;
    let setter = get(c, api, "set")?;
    c.call_rooted(setter, map, &[job, record])?;
    let shared = c.host_mut().shared_state();
    let id = super::async_hooks_shared_vm::create_context(&shared);
    let id_value = c.number(id as f64);
    put(c, record, "asyncId", id_value)?;
    let identity = c.host_weak_root(job)?;
    shared
        .borrow_mut()
        .crypto_secret_jobs
        .resources
        .push(Resource { identity, id });
    #[cfg(quench_openssl_provider)]
    if let Some(request) = request {
        let identity = c.host_weak_root(job)?;
        shared
            .borrow_mut()
            .crypto_secret_jobs
            .configured
            .push(Configured { identity, request });
    }
    super::async_hooks_shared_vm::emit_init(c, job, resource_type, id)?;
    Ok(job)
}
#[cfg(quench_openssl_provider)]
fn invalid_digest(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => {
            let result = (|| {
                let key = c.string_rooted("code");
                let code = c.string_rooted("ERR_CRYPTO_INVALID_DIGEST");
                c.define_data_property_rooted(error, key, code, true, true, true)?;
                Ok::<_, RootedError>(c.throw(error))
            })();
            result.unwrap_or_else(|error| error)
        }
        Err(error) => error,
    }
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_hash(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |i: usize| args.get(i).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    let Some(name) = c.string_text(arg(1))? else {
        return Err(fail(c, "Hash algorithm must be a string"));
    };
    // Native jobs use ncrypto::Digest::FromName, not the public Hash cache.
    // A known legacy descriptor resolves its provider at actual execution;
    // a provider-only algorithm retains the fetched Md it genuinely owns.
    let digest = super::crypto_digest::Digest::from_sdk_name(&name)
        .map(std::sync::Arc::new)
        .map_err(|_| {
            invalid_digest(
                c,
                &format!(
                    "Invalid digest: {}",
                    super::crypto_strings::null_terminated_prefix(&name)
                ),
            )
        })?;
    let Some(bytes) = c.bytes_prefix_rooted(arg(2), usize::MAX) else {
        return Err(fail(c, "Hash input must be a BufferSource"));
    };
    if bytes.len() > i32::MAX as usize {
        let error = c.range_error_rooted("data is too big")?;
        return Err(c.throw(error));
    }
    let length = c
        .rooted_value(arg(3))
        .and_then(|value| value.as_number())
        .filter(|n| *n >= 0.0 && *n <= u32::MAX as f64 && n.fract() == 0.0)
        .map(|n| (n as u32 / u8::BITS) as usize);
    if length.is_some_and(|length| length != digest.size() && !digest.is_xof()) {
        return Err(invalid_digest(c, "Digest method not supported"));
    }
    if args.get(4..).is_some_and(|options| {
        options.iter().any(|value| {
            !c.rooted_value(*value)
                .is_some_and(|value| value.is_undefined())
        })
    }) {
        return Err(invalid_digest(
            c,
            "Customized cSHAKE digest parameters are not supported by this native job",
        ));
    }
    // Async/WebCrypto native jobs own the constructor-time snapshot. Sync jobs
    // retain their original BufferSource and read it when run synchronously.
    let request = HashRequest {
        digest,
        input: (mode != SYNC).then_some(bytes),
        output_length: length,
    };
    let mut fields = vec![("mode", arg(0))];
    if mode == SYNC {
        fields.push(("data", arg(2)));
    }
    register(
        c,
        job,
        &fields,
        "HASHREQUEST",
        Some(NativeRequest::Hash(request)),
    )
}
#[cfg(quench_openssl_provider)]
fn take_request(c: &mut Context<'_>, job: RootId) -> Result<Option<NativeRequest>, RootedError> {
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .crypto_secret_jobs
        .configured
        .iter()
        .map(|job| job.identity)
        .collect::<Vec<_>>();
    for (i, identity) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(identity) {
            if c.strict_equal_rooted(value, job)? {
                let configured = shared.borrow_mut().crypto_secret_jobs.configured.remove(i);
                c.release_host_weak_root(configured.identity);
                return Ok(Some(configured.request));
            }
        }
    }
    Ok(None)
}
#[cfg(quench_openssl_provider)]
pub(crate) fn digest_inventory(c: &mut Context<'_>, aliases: bool) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let synchronized = shared
        .borrow_mut()
        .crypto_secret_jobs
        .digests
        .synchronize(super::crypto_provider_shared_vm::fips_generation);
    synchronized.map_err(|error| native_error(c, Failure::Digest(error)))?;
    if aliases {
        let values = shared
            .borrow()
            .crypto_secret_jobs
            .digests
            .aliases()
            .map(|(name, id)| (name.to_owned(), id))
            .collect::<Vec<_>>();
        let result = c.null_object_rooted()?;
        for (name, id) in values {
            let value = c.number(id as f64);
            put(c, result, &name, value)?;
        }
        Ok(result)
    } else {
        let names = shared
            .borrow()
            .crypto_secret_jobs
            .digests
            .names()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let values = names
            .into_iter()
            .map(|name| c.string_rooted(&name))
            .collect::<Vec<_>>();
        c.array_rooted(&values)
    }
}
pub(crate) fn executor(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let record = c.host_function_data()?;
    put(c, record, "resolve", args[0])?;
    put(c, record, "reject", args[1])?;
    Ok(c.undefined())
}
fn encode(
    c: &mut Context<'_>,
    record: RootId,
    bytes: Material,
    mode: u32,
) -> Result<RootId, RootedError> {
    let algorithm = get(c, record, "algorithm")?;
    let usages = get(c, record, "usages")?;
    let extractable = get(c, record, "extractable")?;
    super::crypto_key_shared_vm::generated_secret(
        c,
        bytes,
        if mode == WEBCRYPTO {
            Some((algorithm, usages, extractable))
        } else {
            None
        },
    )
}
fn encode_output(
    c: &mut Context<'_>,
    record: RootId,
    output: Output,
    mode: u32,
) -> Result<RootId, RootedError> {
    match output {
        #[cfg(quench_openssl_provider)]
        Output::Random(bytes) => {
            let destination = get(c, record, "randomDestination")?;
            let setter = get(c, record, "randomSetter")?;
            super::crypto_provider_shared_vm::random_write(c, destination, setter, &bytes)?;
            Ok(c.undefined())
        }
        #[cfg(quench_openssl_provider)]
        Output::Signature(super::crypto_signature_request::Output::Signature(bytes)) => {
            encode_bytes(c, &bytes)
        }
        #[cfg(quench_openssl_provider)]
        Output::Signature(super::crypto_signature_request::Output::Verified(value)) => {
            Ok(c.boolean(value))
        }
        Output::Key(material) => encode(c, record, material, mode),
        #[cfg(quench_openssl_provider)]
        Output::Digest(bytes) => encode_bytes(c, &bytes),
        #[cfg(quench_openssl_provider)]
        Output::Cipher(bytes) => encode_bytes(c, &bytes),
        #[cfg(quench_openssl_provider)]
        Output::Derived(material) => encode_bytes(c, &material),
        #[cfg(quench_openssl_provider)]
        Output::KeyPair(pair) => encode_pair(c, record, pair, mode),
        #[cfg(quench_openssl_provider)]
        Output::Kem(result) => {
            if mode == WEBCRYPTO {
                let shared_key = encode_bytes(c, &result.shared_key)?;
                if result.decapsulation {
                    return Ok(shared_key);
                }
                let ciphertext = encode_bytes(c, &result.ciphertext)?;
                let output = c.object_rooted()?;
                put(c, output, "sharedKey", shared_key)?;
                put(c, output, "ciphertext", ciphertext)?;
                return Ok(output);
            }
            let shared_key = super::buffer_shared_vm::from_bytes(c, &result.shared_key)?;
            if result.decapsulation {
                return Ok(shared_key);
            }
            let ciphertext = super::buffer_shared_vm::from_bytes(c, &result.ciphertext)?;
            c.array_rooted(&[shared_key, ciphertext])
        }
    }
}
#[cfg(quench_openssl_provider)]
pub(crate) fn encode_bytes(c: &mut Context<'_>, bytes: &[u8]) -> Result<RootId, RootedError> {
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let ctor = get(c, global, "Uint8Array")?;
        let proto = get(c, ctor, "prototype")?;
        let proto = c.prototype_rooted(proto)?;
        let key = c.string_rooted("buffer");
        let descriptor = c.own_property_descriptor_rooted(proto, key)?;
        let getter = get(c, descriptor, "get")?;
        let bytes = c.uint8_array_rooted(bytes)?;
        c.call_rooted(getter, bytes, &[])
    })
}

fn native_error(c: &mut Context<'_>, failure: Failure) -> RootedError {
    match failure {
        #[cfg(quench_openssl_provider)]
        Failure::Pbkdf2(stack) => {
            super::crypto_error_shared_vm::job_failure(c, &stack, "PBKDF2 derivation failed")
        }
        #[cfg(quench_openssl_provider)]
        Failure::Argon2(stack) => {
            super::crypto_error_shared_vm::job_failure(c, &stack, "Argon2 derivation failed")
        }
        #[cfg(quench_openssl_provider)]
        Failure::Sign(super::crypto_asymmetric::KeyError::OpenSsl(stack)) => {
            let reason = stack.errors().iter().find_map(|error| error.reason());
            let text = match reason {
                Some(reason)
                    if reason.eq_ignore_ascii_case("operation not supported for this keytype") =>
                {
                    "operation not supported for this keytype"
                }
                Some(reason) if reason.eq_ignore_ascii_case("digest too big for rsa key") => {
                    "digest too big for rsa key"
                }
                Some(reason) if reason.eq_ignore_ascii_case("invalid digest") => "invalid digest",
                Some(reason) if reason.eq_ignore_ascii_case("digest not allowed") => {
                    "digest not allowed"
                }
                Some(reason) if reason.eq_ignore_ascii_case("pss saltlen too small") => {
                    "pss saltlen too small"
                }
                Some(reason)
                    if reason.eq_ignore_ascii_case("illegal or unsupported padding mode") =>
                {
                    "illegal or unsupported padding mode"
                }
                _ => "Signature job failed",
            };
            super::crypto_error_shared_vm::job_failure(c, &stack, text)
        }
        #[cfg(quench_openssl_provider)]
        Failure::Sign(error) => {
            let message = match error {
                super::crypto_asymmetric::KeyError::Unsupported(message) => message,
                _ => "Signature job failed",
            };
            match c.error_rooted(message) {
                Ok(error) => {
                    let code = c.string_rooted("ERR_CRYPTO_OPERATION_FAILED");
                    match put(c, error, "code", code) {
                        Ok(()) => c.throw(error),
                        Err(error) => error,
                    }
                }
                Err(error) => error,
            }
        }
        #[cfg(quench_openssl_provider)]
        Failure::RsaCipher(super::crypto_asymmetric::KeyError::OpenSsl(stack)) => {
            super::crypto_error_shared_vm::job_failure(c, &stack, "RSA cipher job failed")
        }
        #[cfg(quench_openssl_provider)]
        Failure::RsaCipher(error) => {
            let message = match error {
                super::crypto_asymmetric::KeyError::Unsupported(message) => message,
                _ => "RSA cipher job failed",
            };
            match c.error_rooted(message) {
                Ok(error) => {
                    let code = c.string_rooted("ERR_CRYPTO_OPERATION_FAILED");
                    match put(c, error, "code", code) {
                        Ok(()) => c.throw(error),
                        Err(error) => error,
                    }
                }
                Err(error) => error,
            }
        }
        #[cfg(quench_openssl_provider)]
        Failure::DhBits(stack) => super::crypto_error_shared_vm::job_failure(
            c,
            &stack,
            "Diffie-Hellman derivation failed",
        ),
        #[cfg(quench_openssl_provider)]
        Failure::Hmac(super::crypto_digest::DigestError::OpenSsl(stack)) => {
            super::crypto_error_shared_vm::job_failure(c, &stack, "HMAC operation failed")
        }
        #[cfg(quench_openssl_provider)]
        Failure::Hmac(error) => match c.error_rooted(&error.to_string()) {
            Ok(error) => c.throw(error),
            Err(error) => error,
        },

        Failure::Crypto(stack) => super::crypto_error_shared_vm::decorated(c, stack),
        #[cfg(quench_openssl_provider)]
        Failure::Digest(super::crypto_digest::DigestError::OpenSsl(stack)) => {
            super::crypto_error_shared_vm::decorated(c, stack)
        }
        #[cfg(quench_openssl_provider)]
        Failure::Cipher(stack) => {
            super::crypto_error_shared_vm::job_failure(c, &stack, "AES cipher operation failed")
        }
        #[cfg(quench_openssl_provider)]
        Failure::Digest(error) => match c.error_rooted(&error.to_string()) {
            Ok(error) => c.throw(error),
            Err(error) => error,
        },
        #[cfg(quench_openssl_provider)]
        Failure::Hkdf(stack) => {
            super::crypto_error_shared_vm::job_failure(c, &stack, "HKDF derivation failed")
        }
        #[cfg(quench_openssl_provider)]
        Failure::Kmac(stack) => {
            super::crypto_error_shared_vm::job_failure(c, &stack, "KMAC derivation failed")
        }
        #[cfg(quench_openssl_provider)]
        Failure::KeyPair(super::crypto_asymmetric::KeyError::OpenSsl(stack))
            if !stack.errors().is_empty() =>
        {
            super::crypto_error_shared_vm::decorated(c, stack)
        }
        #[cfg(quench_openssl_provider)]
        Failure::KeyPair(super::crypto_asymmetric::KeyError::Unsupported("unknown cipher")) => {
            match c.error_rooted("Unknown cipher") {
                Ok(error) => {
                    let value = c.string_rooted("ERR_CRYPTO_UNKNOWN_CIPHER");
                    match put(c, error, "code", value) {
                        Ok(()) => c.throw(error),
                        Err(error) => error,
                    }
                }
                Err(error) => error,
            }
        }
        #[cfg(quench_openssl_provider)]
        Failure::KeyPair(_) => match c.error_rooted("Key generation job failed") {
            Ok(error) => c.throw(error),
            Err(error) => error,
        },
        #[cfg(quench_openssl_provider)]
        Failure::Kem { decapsulation, .. } => {
            let message = if decapsulation {
                "Decapsulation failed"
            } else {
                "Encapsulation failed"
            };
            match c.error_rooted(message) {
                Ok(error) => {
                    let code = c.string_rooted("ERR_CRYPTO_OPERATION_FAILED");
                    match put(c, error, "code", code) {
                        Ok(()) => c.throw(error),
                        Err(error) => error,
                    }
                }
                Err(error) => error,
            }
        }
        Failure::WorkerDisconnected => match c.error_rooted("Secret key worker disconnected") {
            Ok(error) => c.throw(error),
            Err(error) => error,
        },
    }
}
fn exception(c: &mut Context<'_>, error: RootedError) -> Result<RootId, RootedError> {
    if let Some(root) = error.exception {
        Ok(root)
    } else {
        c.error_rooted(&error.to_string())
    }
}
fn queue(c: &mut Context<'_>, data: RootId, completion: bool) -> Result<(), RootedError> {
    let callback = c.host_function_with_data(
        crate::host::shared_vm::operation(if completion {
            "cryptoSecretJobComplete"
        } else {
            "cryptoSecretJobPoll"
        }),
        data,
    )?;
    let shared = c.host_mut().shared_state();
    let id = shared
        .borrow_mut()
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("Crypto completion IDs exhausted"))?;
    let resource = if completion {
        let job = get(c, data, "0")?;
        let record = get(c, data, "1")?;
        let id = get(c, record, "asyncId")?;
        let id = c.to_number(id)? as u64;
        Some((job, id))
    } else {
        None
    };
    let callback = c.retain(callback)?;
    let undefined = c.undefined();
    let receiver = match c.retain(undefined) {
        Ok(root) => root,
        Err(error) => {
            c.release_root(callback);
            return Err(error);
        }
    };
    let execution = if let Some((job, id)) = resource {
        let resource = match c.retain(job) {
            Ok(root) => root,
            Err(error) => {
                c.release_root(callback);
                c.release_root(receiver);
                return Err(error);
            }
        };
        Some(super::shared_event_loop::CallbackExecution {
            lifetime: crate::modules::shared_event_loop::CallbackLifetime::Operation,
            optional_callback: None,
            id,
            resource,
        })
    } else {
        None
    };
    shared.borrow_mut().scheduler.queue_shared_immediate(
        id,
        super::shared_event_loop::SharedImmediateOwner::Host,
        super::shared_event_loop::SharedCallback {
            execution,
            callback,
            receiver,
            args: Vec::new(),
        },
    );
    if completion {
        // The canonical scheduler now owns destruction on every return path,
        // including before-hook throws and reentrant/throwing guest callbacks.
        let (_, id) = resource.expect("completion resource");
        let index = shared
            .borrow()
            .crypto_secret_jobs
            .resources
            .iter()
            .position(|r| r.id == id);
        if let Some(index) = index {
            let resource = shared
                .borrow_mut()
                .crypto_secret_jobs
                .resources
                .remove(index);
            c.release_host_weak_root(resource.identity);
        }
    }
    Ok(())
}
pub(crate) fn run(c: &mut Context<'_>, job: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let record = record(c, job)?;
    let started = get(c, record, "started")?;
    if c.truthy_rooted(started)? {
        return Err(fail(c, "Crypto job already started"));
    }
    let mode = get(c, record, "mode")?;
    let mode = uint(c, mode)?;
    #[cfg(quench_openssl_provider)]
    let request = take_request(c, job)?;
    #[cfg(quench_openssl_provider)]
    let work: Box<dyn FnOnce() -> Outcome + Send> = match request {
        Some(NativeRequest::Random(size)) => Box::new(move || {
            let mut bytes = Material::new(vec![0; size]);
            openssl::rand::rand_bytes(&mut bytes).map_err(Failure::Crypto)?;
            Ok(Output::Random(bytes))
        }),
        Some(NativeRequest::Aes(request)) => Box::new(move || {
            aes_cipher(request)
                .map(Output::Cipher)
                .map_err(Failure::Cipher)
        }),
        Some(NativeRequest::RsaCipher(request)) => Box::new(move || {
            let operation = if request.encrypt {
                super::crypto_asymmetric_operations::RsaOperation::PublicEncrypt
            } else {
                super::crypto_asymmetric_operations::RsaOperation::PrivateDecrypt
            };
            let options = super::crypto_asymmetric_operations::RsaOptions {
                padding: openssl::rsa::Padding::PKCS1_OAEP,
                digest: Some(&request.digest),
                mgf1_digest: Some(&request.digest),
                label: (!request.label.is_empty()).then_some(request.label.as_slice()),
            };
            super::crypto_asymmetric_operations::rsa(
                &request.key,
                operation,
                &request.data,
                &options,
            )
            .map(Output::Cipher)
            .map_err(Failure::RsaCipher)
        }),
        Some(NativeRequest::DhBits(request)) => Box::new(move || {
            let private_key = request
                .private_key
                .private_ref()
                .ok_or_else(|| Failure::DhBits(openssl::error::ErrorStack::get()))?;
            let mut derive = openssl::derive::Deriver::new(private_key).map_err(Failure::DhBits)?;
            derive
                .set_peer(request.public_key.public_ref())
                .map_err(Failure::DhBits)?;
            let secret = derive.derive_to_vec().map_err(Failure::DhBits)?;
            Ok(Output::Derived(Material::new(secret)))
        }),
        Some(NativeRequest::Pbkdf2(mut request)) => {
            let password = match request.password.take() {
                Some(bytes) => bytes,
                None => std::sync::Arc::new(Material::new(request_bytes(c, record, "password")?)),
            };
            let salt = match request.salt.take() {
                Some(bytes) => bytes,
                None => request_bytes(c, record, "salt")?,
            };
            Box::new(move || {
                super::crypto_pbkdf2::derive(
                    &request.digest,
                    &password,
                    &salt,
                    request.iterations,
                    request.length,
                )
                .map(Output::Derived)
                .map_err(Failure::Pbkdf2)
            })
        }
        Some(NativeRequest::Argon2(request)) => Box::new(move || {
            super::crypto_argon2::derive(
                request.algorithm,
                &request.message,
                &request.nonce,
                request.parallelism,
                request.tag_length,
                request.memory,
                request.passes,
                &request.secret,
                &request.associated_data,
            )
            .map(Output::Derived)
            .map_err(Failure::Argon2)
        }),
        Some(NativeRequest::Sign(mut sign)) => {
            let data = match sign.data.take() {
                Some(data) => data,
                None => request_bytes(c, record, "data")?,
            };
            let signature = if sign.verify {
                match sign.signature.take() {
                    Some(bytes) => bytes,
                    None => request_bytes(c, record, "signature")?,
                }
            } else {
                Vec::new()
            };
            if sign.context.is_none() {
                let root = get(c, record, "context")?;
                sign.context = c.bytes_prefix_rooted(root, usize::MAX);
            }
            Box::new(move || {
                if sign.context.as_ref().is_some_and(|bytes| !bytes.is_empty())
                    && !super::crypto_signature_request::supports_context(&sign.key)
                {
                    return Err(Failure::Sign(
                        super::crypto_asymmetric::KeyError::Unsupported(
                            "Context parameter is unsupported",
                        ),
                    ));
                }
                sign.run(data, signature)
                    .map(Output::Signature)
                    .map_err(Failure::Sign)
            })
        }

        Some(NativeRequest::Hmac(request)) => Box::new(move || {
            let mac = request
                .digest
                .hmac(&request.key, &request.data)
                .map_err(Failure::Hmac)?;
            let output = if let Some(expected) = request.signature {
                let valid = !mac.is_empty()
                    && mac.len() == expected.len()
                    && openssl::memcmp::eq(&mac, &expected);
                super::crypto_signature_request::Output::Verified(valid)
            } else {
                super::crypto_signature_request::Output::Signature(mac)
            };
            Ok(Output::Signature(output))
        }),

        Some(NativeRequest::Kmac(request)) => {
            Box::new(move || request.run().map(Output::Signature).map_err(Failure::Kmac))
        }
        Some(NativeRequest::Kem(request)) => {
            let decapsulation = request.is_decapsulation();
            Box::new(move || {
                request
                    .run()
                    .map(Output::Kem)
                    .map_err(|_| Failure::Kem { decapsulation })
            })
        }

        Some(NativeRequest::Hash(mut hash)) => {
            let input = match hash.input.take() {
                Some(input) => input,
                None => request_bytes(c, record, "data")?,
            };
            Box::new(move || {
                hash.digest
                    .hash(&input, hash.output_length)
                    .map(Output::Digest)
                    .map_err(Failure::Digest)
            })
        }
        Some(NativeRequest::Hkdf(mut hkdf)) => {
            let key = match hkdf.key.take() {
                Some(key) => key,
                None => std::sync::Arc::new(Material::new(request_bytes(c, record, "key")?)),
            };
            let salt = match hkdf.salt.take() {
                Some(salt) => salt,
                None => request_bytes(c, record, "salt")?,
            };
            let info = match hkdf.info.take() {
                Some(info) => info,
                None => request_bytes(c, record, "info")?,
            };
            Box::new(move || {
                super::crypto_hkdf::derive(&hkdf.digest, &key, &salt, &info, hkdf.length)
                    .map(Output::Derived)
                    .map_err(Failure::Hkdf)
            })
        }
        Some(NativeRequest::KeyPair(request)) => {
            Box::new(move || request.run().map(Output::KeyPair).map_err(Failure::KeyPair))
        }
        None => {
            let bits = get(c, record, "bits")?;
            let bits = uint(c, bits)?;
            Box::new(move || generate(bits, mode))
        }
    };
    #[cfg(not(quench_openssl_provider))]
    let work: Box<dyn FnOnce() -> Outcome + Send> = {
        let bits = get(c, record, "bits")?;
        let bits = uint(c, bits)?;
        Box::new(move || generate(bits, mode))
    };
    let yes = c.boolean(true);
    put(c, record, "started", yes)?;
    if mode == SYNC {
        let result = work().map_err(|failure| native_error(c, failure));
        let result = result.and_then(|output| encode_output(c, record, output, mode));
        let missing = c.undefined();
        return match result {
            Ok(key) => c.array_rooted(&[missing, key]),
            Err(error) => {
                let error = exception(c, error)?;
                c.array_rooted(&[error, missing])
            }
        };
    }
    let promise = if mode == WEBCRYPTO {
        let api = api(c)?;
        let ctor = get(c, api, "Promise")?;
        let executor = c.host_function_with_data(
            crate::host::shared_vm::operation("cryptoSecretJobExecutor"),
            record,
        )?;
        c.construct_rooted(ctor, ctor, &[executor])?
    } else {
        c.undefined()
    };
    // A queued native callback owns job -> WeakMap record -> algorithm/resolvers.
    // The worker receives only native values; it never touches VM roots.
    let data = c.array_rooted(&[job, record])?;
    let identity = c.host_weak_root(job)?;
    let (sender, receiver) = mpsc::channel();
    let shared = c.host_mut().shared_state();
    let worker = shared.borrow_mut().native_tasks.submit(move || {
        let result = work();
        let _ = sender.send(result);
    });
    if let Err(error) = worker {
        c.release_host_weak_root(identity);
        return Err(RootedError::host(error.to_string()));
    }
    let shared = c.host_mut().shared_state();
    shared
        .borrow_mut()
        .crypto_secret_jobs
        .running
        .push(Running {
            identity,
            result: Delivery::Pending(receiver),
        });
    if let Err(error) = queue(c, data, false) {
        let entry = shared
            .borrow_mut()
            .crypto_secret_jobs
            .running
            .pop()
            .expect("just inserted");
        c.release_host_weak_root(entry.identity);
        return Err(error);
    }
    Ok(promise)
}
pub(crate) fn poll(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let job = get(c, data, "0")?;
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .crypto_secret_jobs
        .running
        .iter()
        .map(|e| e.identity)
        .collect::<Vec<_>>();
    let mut index = None;
    for (i, key) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(key) {
            if c.strict_equal_rooted(value, job)? {
                index = Some(i);
                break;
            }
        }
    }
    let Some(index) = index else {
        return Err(RootedError::host("Missing running crypto job"));
    };
    let result = {
        let state = shared.borrow();
        match &state.crypto_secret_jobs.running[index].result {
            Delivery::Pending(receiver) => receiver.try_recv(),
            Delivery::Ready(_) => {
                return Err(RootedError::host("Crypto completion already queued"));
            }
        }
    };
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(TryRecvError::Empty) => {
            queue(c, data, false)?;
            return Ok(c.undefined());
        }
        Err(TryRecvError::Disconnected) => {
            // Preserve disconnected worker failure as a host error, not empty material.
            shared.borrow_mut().crypto_secret_jobs.running[index].result =
                Delivery::Ready(Err(Failure::WorkerDisconnected));
            queue(c, data, true)?;
            return Ok(c.undefined());
        }
    };
    shared.borrow_mut().crypto_secret_jobs.running[index].result = Delivery::Ready(outcome);
    queue(c, data, true)?;
    Ok(c.undefined())
}
pub(crate) fn complete(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let job = get(c, data, "0")?;
    let record = get(c, data, "1")?;
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .crypto_secret_jobs
        .running
        .iter()
        .map(|e| e.identity)
        .collect::<Vec<_>>();
    let mut index = None;
    for (i, key) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(key) {
            if c.strict_equal_rooted(value, job)? {
                index = Some(i);
                break;
            }
        }
    }
    let index = index.ok_or_else(|| RootedError::host("Missing completed crypto job"))?;
    let entry = shared.borrow_mut().crypto_secret_jobs.running.remove(index);
    c.release_host_weak_root(entry.identity);
    let Delivery::Ready(outcome) = entry.result else {
        return Err(RootedError::host("Crypto completion before worker result"));
    };
    let mode = get(c, record, "mode")?;
    let mode = uint(c, mode)?;
    let result = match outcome {
        Ok(output) => encode_output(c, record, output, mode),
        Err(failure) => Err(native_error(c, failure)),
    };
    let undefined = c.undefined();
    if mode == ASYNC {
        let callback = get(c, job, "ondone")?;
        let (error, key) = match result {
            Ok(key) => (undefined, key),
            Err(e) => (exception(c, e)?, undefined),
        };
        c.call_rooted(callback, job, &[error, key])?;
    } else {
        match result {
            Ok(key) => {
                let resolve = get(c, record, "resolve")?;
                if c.is_object_rooted(key)? {
                    // Suppress inherited then hooks during intrinsic promise
                    // resolution for object results (CryptoKeys and byte arrays).
                    let then = c.string_rooted("then");
                    c.define_data_property_rooted(key, then, undefined, true, false, true)?;
                    let settled = c.call_rooted(resolve, undefined, &[key]);
                    let api = api(c)?;
                    let delete = get(c, api, "delete")?;
                    let cleanup = c.call_rooted(delete, undefined, &[key, then]);
                    settled?;
                    cleanup?;
                } else {
                    c.call_rooted(resolve, undefined, &[key])?;
                }
            }
            Err(error) => {
                let cause = exception(c, error)?;
                let api = api(c)?;
                let ctor = get(c, api, "DOMException")?;
                let options = c.null_object_rooted()?;
                let name = c.string_rooted("OperationError");
                put(c, options, "name", name)?;
                put(c, options, "cause", cause)?;
                let message =
                    c.string_rooted("The operation failed for an operation-specific reason");
                let error = c.construct_rooted(ctor, ctor, &[message, options])?;
                let reject = get(c, record, "reject")?;
                c.call_rooted(reject, undefined, &[error])?;
            }
        }
        put(c, record, "resolve", undefined)?;
        put(c, record, "reject", undefined)?;
    }
    Ok(undefined)
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    #[cfg(quench_openssl_provider)]
    {
        let dead = shared
            .borrow()
            .crypto_secret_jobs
            .configured
            .iter()
            .enumerate()
            .filter_map(|(i, job)| (!runtime.host_weak_root_is_live(job.identity)).then_some(i))
            .collect::<Vec<_>>();
        for index in dead.into_iter().rev() {
            let job = shared
                .borrow_mut()
                .crypto_secret_jobs
                .configured
                .remove(index);
            runtime.release_host_weak_root(job.identity);
        }
    }
    let dead = shared
        .borrow()
        .crypto_secret_jobs
        .resources
        .iter()
        .enumerate()
        .filter_map(|(i, r)| (!runtime.host_weak_root_is_live(r.identity)).then_some(i))
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let resource = shared
            .borrow_mut()
            .crypto_secret_jobs
            .resources
            .remove(index);
        runtime.release_host_weak_root(resource.identity);
        super::async_hooks_shared_vm::complete_runtime_context(runtime, resource.id);
    }
    let dead = shared
        .borrow()
        .crypto_secret_jobs
        .running
        .iter()
        .enumerate()
        .filter_map(|(i, r)| (!runtime.host_weak_root_is_live(r.identity)).then_some(i))
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let entry = shared.borrow_mut().crypto_secret_jobs.running.remove(index);
        runtime.release_host_weak_root(entry.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    #[cfg(quench_openssl_provider)]
    {
        let hashes = std::mem::take(&mut shared.borrow_mut().crypto_secret_jobs.configured);
        for job in hashes {
            runtime.release_host_weak_root(job.identity);
        }
        shared.borrow_mut().crypto_secret_jobs.digests = Default::default();
    }
    let entries = std::mem::take(&mut shared.borrow_mut().crypto_secret_jobs.running);
    for entry in entries {
        runtime.release_host_weak_root(entry.identity);
    }
    let resources = std::mem::take(&mut shared.borrow_mut().crypto_secret_jobs.resources);
    for resource in resources {
        runtime.release_host_weak_root(resource.identity);
        super::async_hooks_shared_vm::clear_context(runtime, &shared, resource.id);
    }
    // Dropping receivers causes late worker results to be dropped and cleansed.
}

#[cfg(quench_openssl_provider)]
fn request_bytes(c: &mut Context<'_>, record: RootId, name: &str) -> Result<Vec<u8>, RootedError> {
    let value = get(c, record, name)?;
    c.bytes_prefix_rooted(value, usize::MAX)
        .ok_or_else(|| fail(c, "Crypto job input is detached"))
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_hkdf(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    let name = c
        .string_text(arg(1))?
        .ok_or_else(|| fail(c, "HKDF digest must be a string"))?;
    let digest = super::crypto_digest::Digest::from_sdk_name(&name)
        .map(std::sync::Arc::new)
        .map_err(|_| {
            invalid_digest(
                c,
                &format!(
                    "Invalid digest: {}",
                    super::crypto_strings::null_terminated_prefix(&name)
                ),
            )
        })?;
    let (key, is_key_handle) = match c.bytes_prefix_rooted(arg(2), usize::MAX) {
        Some(bytes) => (std::sync::Arc::new(Material::new(bytes)), false),
        None => (super::crypto_key_shared_vm::secret_bytes(c, arg(2))?, true),
    };
    let salt = c
        .bytes_prefix_rooted(arg(3), usize::MAX)
        .ok_or_else(|| fail(c, "HKDF salt must be a BufferSource"))?;
    let info = c
        .bytes_prefix_rooted(arg(4), usize::MAX)
        .ok_or_else(|| fail(c, "HKDF info must be a BufferSource"))?;
    for (name, length) in [
        ("key", key.len()),
        ("salt", salt.len()),
        ("info", info.len()),
    ] {
        if length > i32::MAX as usize {
            let error = c.range_error_rooted(&format!("{name} is too big"))?;
            let code = c.string_rooted("ERR_OUT_OF_RANGE");
            put(c, error, "code", code)?;
            return Err(c.throw(error));
        }
    }
    let length = uint(c, arg(5))? as usize;
    if length > super::crypto_hkdf::maximum(&digest) {
        let error = c.range_error_rooted("Invalid key length")?;
        let code = c.string_rooted("ERR_CRYPTO_INVALID_KEYLEN");
        put(c, error, "code", code)?;
        return Err(c.throw(error));
    }
    let request = HkdfRequest {
        digest,
        key: (mode != SYNC || is_key_handle).then_some(key),
        salt: (mode != SYNC).then_some(salt),
        info: (mode != SYNC).then_some(info),
        length,
    };
    let mut fields = vec![("mode", arg(0))];
    if mode == SYNC {
        if !is_key_handle {
            fields.push(("key", arg(2)));
        }
        fields.push(("salt", arg(3)));
        fields.push(("info", arg(4)));
    }
    register(
        c,
        job,
        &fields,
        "DERIVEBITSREQUEST",
        Some(NativeRequest::Hkdf(request)),
    )
}

#[cfg(quench_openssl_provider)]
fn create_pair(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
    family: super::crypto_keypair_request::Family,
) -> Result<RootId, RootedError> {
    let (request, fields) = super::crypto_keypair_config::configure(c, family, args)?;
    register(
        c,
        job,
        &fields,
        "KEYPAIRGENREQUEST",
        Some(NativeRequest::KeyPair(request)),
    )
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_rsa_pair(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    create_pair(c, job, args, super::crypto_keypair_request::Family::Rsa)
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_ec_pair(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    create_pair(c, job, args, super::crypto_keypair_request::Family::Ec)
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_dsa_pair(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    create_pair(c, job, args, super::crypto_keypair_request::Family::Dsa)
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_dh_pair(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    create_pair(c, job, args, super::crypto_keypair_request::Family::Dh)
}
#[cfg(quench_openssl_provider)]
pub(crate) fn create_named_pair(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    create_pair(c, job, args, super::crypto_keypair_request::Family::Named)
}
#[cfg(quench_openssl_provider)]
fn encode_pair(
    c: &mut Context<'_>,
    record: RootId,
    pair: super::crypto_keypair_request::Pair,
    mode: u32,
) -> Result<RootId, RootedError> {
    if mode == WEBCRYPTO {
        let algorithm = get(c, record, "algorithm")?;
        let pub_usage = get(c, record, "publicUsages")?;
        let priv_usage = get(c, record, "privateUsages")?;
        let extractable = get(c, record, "extractable")?;
        let yes = c.boolean(true);
        let public = super::crypto_key_shared_vm::generated_asymmetric(
            c,
            pair.public,
            Some((algorithm, pub_usage, yes)),
        )?;
        let private = super::crypto_key_shared_vm::generated_asymmetric(
            c,
            pair.private,
            Some((algorithm, priv_usage, extractable)),
        )?;
        let result = c.object_rooted()?;
        put(c, result, "publicKey", public)?;
        put(c, result, "privateKey", private)?;
        return Ok(result);
    }
    let public = encode_pair_key(c, pair.public, pair.encoding.public, None, None)?;
    let private = encode_pair_key(
        c,
        pair.private,
        pair.encoding.private,
        pair.encoding.cipher.as_deref(),
        pair.encoding.passphrase.as_deref(),
    )?;
    c.array_rooted(&[public, private])
}
#[cfg(quench_openssl_provider)]
fn encode_pair_key(
    c: &mut Context<'_>,
    key: std::sync::Arc<super::crypto_asymmetric::OwnedKey>,
    encoding: Option<super::crypto_keypair_request::OutputEncoding>,
    cipher: Option<&str>,
    passphrase: Option<&[u8]>,
) -> Result<RootId, RootedError> {
    use super::crypto_keypair_request::OutputEncoding;
    let Some(encoding) = encoding else {
        return super::crypto_key_shared_vm::generated_asymmetric(c, key, None);
    };
    let (format, encoding) = match encoding {
        OutputEncoding::Bytes(format, encoding) => (format, encoding),
        OutputEncoding::Jwk => {
            let target = c.object_rooted()?;
            return super::crypto_asymmetric_shared_vm::export_owned_jwk(c, &key, target, false);
        }
        OutputEncoding::RawPublic { .. } | OutputEncoding::RawPrivate => {
            let bytes = Material::new(
                match encoding {
                    OutputEncoding::RawPublic { compressed } => key.raw_public(compressed),
                    _ => key.raw_private(),
                }
                .map_err(|error| native_error(c, Failure::KeyPair(error)))?,
            );
            return super::buffer_shared_vm::from_bytes(c, &bytes);
        }
        OutputEncoding::RawSeed => {
            return Err(fail(c, "Native raw seed encoding is not yet implemented"));
        }
    };
    let bytes = Material::new(
        key.export(format, encoding, cipher, passphrase)
            .map_err(|e| native_error(c, Failure::KeyPair(e)))?,
    );
    if format == super::crypto_asymmetric::Format::Pem {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| RootedError::host("Native PEM is not UTF-8"))?;
        Ok(c.string_rooted(text))
    } else {
        super::buffer_shared_vm::from_bytes(c, &bytes)
    }
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_sign(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |n: usize| args.get(n).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    let verify = uint(c, arg(1))? == super::crypto_signature_request::Mode::Verify as u32;
    let target = c.number(if verify { 1.0 } else { 2.0 });
    let input = [target, arg(2), arg(3), arg(4), arg(5), arg(6)];
    let key = super::crypto_asymmetric_shared_vm::import_input(
        c,
        if verify {
            super::crypto_asymmetric::KeyKind::Public
        } else {
            super::crypto_asymmetric::KeyKind::Private
        },
        &input,
    )?;
    let data = c
        .bytes_prefix_rooted(arg(7), usize::MAX)
        .ok_or_else(|| fail(c, "Signature data must be a BufferSource"))?;
    let digest = if let Some(name) = c.string_text(arg(8))? {
        Some(std::sync::Arc::new(
            super::crypto_digest::Digest::from_sdk_name(&name).map_err(|_| {
                let error = c.type_error_rooted(&format!("Invalid digest: {name}"));
                match error {
                    Ok(error) => {
                        let code = c.string_rooted("ERR_CRYPTO_INVALID_DIGEST");
                        match put(c, error, "code", code) {
                            Ok(()) => c.throw(error),
                            Err(error) => error,
                        }
                    }
                    Err(error) => error,
                }
            })?,
        ))
    } else {
        None
    };
    let number = |c: &mut Context<'_>, root: RootId| -> Result<Option<i32>, RootedError> {
        if c.rooted_value(root)
            .is_some_and(|value| value.is_undefined())
        {
            Ok(None)
        } else {
            Ok(Some(c.to_number(root)? as i32))
        }
    };
    let salt_length = number(c, arg(9))?;
    let padding = number(c, arg(10))?.map(openssl::rsa::Padding::from_raw);
    let mut ieee_p1363 =
        number(c, arg(11))? == Some(super::crypto_signature_request::Encoding::P1363 as i32);
    let context = c.bytes_prefix_rooted(arg(12), usize::MAX);
    if context.as_ref().is_some_and(|bytes| {
        bytes.len() > super::crypto_signature_request::MAX_CONTEXT_STRING_BYTES
    }) {
        let error = c.range_error_rooted("context string must be at most 255 bytes")?;
        let code = c.string_rooted("ERR_OUT_OF_RANGE");
        put(c, error, "code", code)?;
        return Err(c.throw(error));
    }
    let mut signature = if verify {
        Some(
            c.bytes_prefix_rooted(arg(13), usize::MAX)
                .ok_or_else(|| fail(c, "Signature must be a BufferSource"))?,
        )
    } else {
        None
    };
    let converted = verify
        && ieee_p1363
        && matches!(
            key.public_ref().id(),
            openssl::pkey::Id::EC | openssl::pkey::Id::DSA
        );
    if converted {
        signature = Some(
            super::crypto_asymmetric_operations::from_p1363(
                key.public_ref(),
                signature.as_ref().unwrap(),
            )
            .map_err(|error| native_error(c, Failure::Sign(error)))?
            .unwrap_or_default(),
        );
        ieee_p1363 = false;
    }

    let request = super::crypto_signature_request::Request {
        key,
        digest,
        data: (mode != SYNC).then_some(data),
        signature: if mode != SYNC || converted {
            signature
        } else {
            None
        },
        verify,
        options: super::crypto_asymmetric_operations::SignatureOptions {
            padding,
            salt_length,
            ieee_p1363,
        },
        context: if mode != SYNC { context } else { None },
    };
    let mut fields = vec![("mode", arg(0))];
    if mode == SYNC {
        fields.push(("data", arg(7)));
        fields.push(("signature", arg(13)));
        fields.push(("context", arg(12)));
    }
    register(
        c,
        job,
        &fields,
        "SIGNREQUEST",
        Some(NativeRequest::Sign(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_hmac(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    let verify = match uint(c, arg(1))? {
        value if value == super::crypto_signature_request::Mode::Sign as u32 => false,
        value if value == super::crypto_signature_request::Mode::Verify as u32 => true,
        _ => return Err(fail(c, "Invalid HMAC operation")),
    };
    let name = c
        .string_text(arg(2))?
        .ok_or_else(|| fail(c, "HMAC digest must be a string"))?;
    let digest = super::crypto_digest::Digest::from_sdk_name(&name)
        .map(std::sync::Arc::new)
        .map_err(|_| {
            invalid_digest(
                c,
                &format!(
                    "Invalid digest: {}",
                    super::crypto_strings::null_terminated_prefix(&name)
                ),
            )
        })?;
    let key = super::crypto_key_shared_vm::secret_bytes(c, arg(3))?;
    let data = c
        .bytes_prefix_rooted(arg(4), i32::MAX as usize)
        .ok_or_else(|| fail(c, "HMAC data must be a BufferSource"))?;
    let signature = if c
        .rooted_value(arg(5))
        .is_some_and(|value| value.is_undefined())
    {
        None
    } else {
        Some(
            c.bytes_prefix_rooted(arg(5), i32::MAX as usize)
                .ok_or_else(|| fail(c, "HMAC signature must be a BufferSource"))?,
        )
    };
    let request = HmacRequest {
        digest,
        key,
        data,
        signature: if verify { signature } else { None },
    };
    register(
        c,
        job,
        &[("mode", arg(0))],
        "SIGNREQUEST",
        Some(NativeRequest::Hmac(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_kem_encapsulate(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode != ASYNC && mode != SYNC && mode != WEBCRYPTO {
        return Err(fail(c, "Invalid KEM crypto job mode"));
    }
    let key = super::crypto_asymmetric_shared_vm::import_input(
        c,
        super::crypto_asymmetric::KeyKind::Public,
        args,
    )?;
    let request = kem_request::Request::encapsulate(key);
    register(
        c,
        job,
        &[("mode", arg(0))],
        "KEM_ENCAPSULATEJOB",
        Some(NativeRequest::Kem(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_kem_decapsulate(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode != ASYNC && mode != SYNC && mode != WEBCRYPTO {
        return Err(fail(c, "Invalid KEM crypto job mode"));
    }
    let key = super::crypto_asymmetric_shared_vm::import_input(
        c,
        super::crypto_asymmetric::KeyKind::Private,
        args,
    )?;
    let ciphertext = c
        .bytes_prefix_rooted(arg(6), i32::MAX as usize)
        .ok_or_else(|| fail(c, "KEM ciphertext must be a BufferSource"))?;
    let request = kem_request::Request::decapsulate(key, ciphertext);
    register(
        c,
        job,
        &[("mode", arg(0))],
        "KEM_DECAPSULATEJOB",
        Some(NativeRequest::Kem(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_kmac(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    let verify = match uint(c, arg(1))? {
        value if value == super::crypto_signature_request::Mode::Sign as u32 => false,
        value if value == super::crypto_signature_request::Mode::Verify as u32 => true,
        _ => return Err(fail(c, "Invalid KMAC operation")),
    };
    let key = super::crypto_key_shared_vm::secret_bytes(c, arg(2))?;
    let variant = match c.string_text(arg(3))?.as_deref() {
        Some("KMAC128") => kmac_request::Variant::Kmac128,
        Some("KMAC256") => kmac_request::Variant::Kmac256,
        _ => return Err(fail(c, "Invalid KMAC algorithm")),
    };
    let customization = if c
        .rooted_value(arg(4))
        .is_some_and(|value| value.is_undefined())
    {
        Vec::new()
    } else {
        c.bytes_prefix_rooted(arg(4), i32::MAX as usize)
            .ok_or_else(|| fail(c, "KMAC customization must be a BufferSource"))?
    };
    let output_bits = uint(c, arg(5))?;
    if output_bits % u8::BITS != 0 {
        return Err(fail(c, "Invalid KMAC output length"));
    }
    let data = c
        .bytes_prefix_rooted(arg(6), i32::MAX as usize)
        .ok_or_else(|| fail(c, "KMAC data must be a BufferSource"))?;
    let signature = if c
        .rooted_value(arg(7))
        .is_some_and(|value| value.is_undefined())
    {
        None
    } else {
        Some(
            c.bytes_prefix_rooted(arg(7), i32::MAX as usize)
                .ok_or_else(|| fail(c, "KMAC signature must be a BufferSource"))?,
        )
    };
    let request = kmac_request::Request {
        key,
        variant,
        data,
        customization,
        output_length: (output_bits / u8::BITS) as usize,
        signature: if verify { signature } else { None },
    };
    register(
        c,
        job,
        &[("mode", arg(0))],
        "SIGNREQUEST",
        Some(NativeRequest::Kmac(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_pbkdf2(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    let (password, handle) = match c.bytes_prefix_rooted(arg(1), usize::MAX) {
        Some(bytes) => (std::sync::Arc::new(Material::new(bytes)), false),
        None => (super::crypto_key_shared_vm::secret_bytes(c, arg(1))?, true),
    };
    let salt = c
        .bytes_prefix_rooted(arg(2), usize::MAX)
        .ok_or_else(|| fail(c, "PBKDF2 salt must be a BufferSource"))?;
    for (name, length) in [("pass", password.len()), ("salt", salt.len())] {
        if length > i32::MAX as usize {
            let error = c.range_error_rooted(&format!("{name} is too large"))?;
            let code = c.string_rooted("ERR_OUT_OF_RANGE");
            put(c, error, "code", code)?;
            return Err(c.throw(error));
        }
    }
    let iterations = uint(c, arg(3))?;
    let length = uint(c, arg(4))?;
    for (name, value) in [("iterations", iterations), ("length", length)] {
        if value > i32::MAX as u32 {
            let error = c.range_error_rooted(&format!("{name} must be <= {}", i32::MAX))?;
            let code = c.string_rooted("ERR_OUT_OF_RANGE");
            put(c, error, "code", code)?;
            return Err(c.throw(error));
        }
    }
    let name = c
        .string_text(arg(5))?
        .ok_or_else(|| fail(c, "PBKDF2 digest must be a string"))?;
    let digest = super::crypto_digest::Digest::from_sdk_name(&name)
        .map(std::sync::Arc::new)
        .map_err(|_| {
            invalid_digest(
                c,
                &format!(
                    "Invalid digest: {}",
                    super::crypto_strings::null_terminated_prefix(&name)
                ),
            )
        })?;
    let request = Pbkdf2Request {
        digest,
        password: (mode != SYNC || handle).then_some(password),
        salt: (mode != SYNC).then_some(salt),
        iterations,
        length: length as usize,
    };
    let mut fields = vec![("mode", arg(0))];
    if mode == SYNC {
        if !handle {
            fields.push(("password", arg(1)));
        }
        fields.push(("salt", arg(2)));
    }
    register(
        c,
        job,
        &fields,
        "PBKDF2REQUEST",
        Some(NativeRequest::Pbkdf2(request)),
    )
}

#[cfg(quench_openssl_provider)]
pub(crate) fn create_argon2(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |index: usize| args.get(index).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    let message = match c.bytes_prefix_rooted(arg(1), i32::MAX as usize) {
        Some(bytes) => std::sync::Arc::new(Material::new(bytes)),
        None => super::crypto_key_shared_vm::secret_bytes(c, arg(1))?,
    };
    let nonce = c
        .bytes_prefix_rooted(arg(2), i32::MAX as usize)
        .ok_or_else(|| fail(c, "Argon2 salt must be a BufferSource"))?;
    let parallelism = uint(c, arg(3))?;
    let tag_length = uint(c, arg(4))? as usize;
    let memory = uint(c, arg(5))?;
    let passes = uint(c, arg(6))?;
    let secret = c
        .bytes_prefix_rooted(arg(7), i32::MAX as usize)
        .ok_or_else(|| fail(c, "Argon2 secret must be a BufferSource"))?;
    let associated_data = c
        .bytes_prefix_rooted(arg(8), i32::MAX as usize)
        .ok_or_else(|| fail(c, "Argon2 associated data must be a BufferSource"))?;
    let algorithm = uint(c, arg(9))?;
    if algorithm > 2 {
        return Err(fail(c, "Invalid Argon2 algorithm type"));
    }
    register(
        c,
        job,
        &[("mode", arg(0))],
        "ARGON2REQUEST",
        Some(NativeRequest::Argon2(Argon2Request {
            algorithm,
            message,
            nonce,
            parallelism,
            tag_length,
            memory,
            passes,
            secret,
            associated_data,
        })),
    )
}
