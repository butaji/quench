//! The `crypto` module object: its functions, classes and properties, built on first use.
use super::{call_method, cipher, dh, get, keys, put, sign, state, web, x509, Class, Context, Intrinsics, Outcome};
use quench_runtime::{DescriptorFields, RootId, RootedError};

/// The `stream` module's class `name` (`Transform`, `Writable`).
fn stream_class(c: &mut Context<'_>, name: &str) -> Outcome {
    let stream = crate::modules::stream_shared_vm::module(c)?;
    get(c, stream, name)
}

/// The `crypto` module's functions: `(export name, host operation, function name)`.
const FUNCTIONS: &[(&str, &str, &str)] = &[
    ("checkPrime", "cryptoCheckPrime", "checkPrime"),
    ("checkPrimeSync", "cryptoCheckPrimeSync", "checkPrimeSync"),
    ("createCipheriv", "cryptoCreateCipheriv", "createCipheriv"),
    ("createDecipheriv", "cryptoCreateDecipheriv", "createDecipheriv"),
    ("createDiffieHellman", "cryptoCreateDiffieHellman", "createDiffieHellman"),
    ("createDiffieHellmanGroup", "cryptoCreateDiffieHellmanGroup", "createDiffieHellmanGroup"),
    ("createECDH", "cryptoCreateEcdh", "createECDH"),
    ("createPrivateKey", "cryptoCreatePrivateKey", "createPrivateKey"),
    ("createPublicKey", "cryptoCreatePublicKey", "createPublicKey"),
    ("createSecretKey", "cryptoCreateSecretKey", "createSecretKey"),
    ("createSign", "cryptoCreateSign", "createSign"),
    ("createVerify", "cryptoCreateVerify", "createVerify"),
    ("diffieHellman", "cryptoDiffieHellmanStateless", "diffieHellman"),
    ("generatePrime", "cryptoGeneratePrime", "generatePrime"),
    ("generatePrimeSync", "cryptoGeneratePrimeSync", "generatePrimeSync"),
    ("getCiphers", "cryptoGetCiphers", ""),
    ("getCipherInfo", "cryptoGetCipherInfo", "getCipherInfo"),
    ("getCurves", "cryptoGetCurves", ""),
    ("getDiffieHellman", "cryptoCreateDiffieHellmanGroup", "createDiffieHellmanGroup"),
    ("getHashes", "cryptoGetHashes", ""),
    ("hkdf", "cryptoHkdfAsync", "hkdf"),
    ("hkdfSync", "cryptoHkdfSync", "hkdfSync"),
    ("pbkdf2", "cryptoPbkdf2Async", "pbkdf2"),
    ("pbkdf2Sync", "cryptoPbkdf2Sync", "pbkdf2Sync"),
    ("generateKeyPair", "cryptoGenerateKeyPair", "generateKeyPair"),
    ("generateKeyPairSync", "cryptoGenerateKeyPairSync", "generateKeyPairSync"),
    ("generateKey", "cryptoGenerateKey", "generateKey"),
    ("generateKeySync", "cryptoGenerateKeySync", "generateKeySync"),
    ("privateDecrypt", "cryptoPrivateDecrypt", ""),
    ("privateEncrypt", "cryptoPrivateEncrypt", ""),
    ("publicDecrypt", "cryptoPublicDecrypt", ""),
    ("publicEncrypt", "cryptoPublicEncrypt", ""),
    ("randomBytes", "cryptoRandomBytesApi", "randomBytes"),
    ("randomFill", "cryptoRandomFill", "randomFill"),
    ("randomFillSync", "cryptoRandomFillSync", "randomFillSync"),
    ("randomInt", "cryptoRandomInt", "randomInt"),
    ("randomUUID", "cryptoRandomUuid", "randomUUID"),
    ("scrypt", "cryptoScryptAsync", "scrypt"),
    ("scryptSync", "cryptoScryptSync", "scryptSync"),
    ("sign", "cryptoSignOneShot", "signOneShot"),
    ("setEngine", "cryptoSetEngineApi", "setEngine"),
    ("timingSafeEqual", "cryptoTimingSafeEqual", ""),
    ("getFips", "cryptoGetFips", "getFips"),
    ("setFips", "cryptoSetFips", "setFips"),
    ("verify", "cryptoVerifyOneShot", "verifyOneShot"),
];

/// The module's class members, by export name; the removed legacy `Cipher` and
/// `Decipher` remain own members holding `undefined`.
const CLASS_EXPORTS: &[(&str, Option<Class>)] = &[
    ("Certificate", Some(Class::Certificate)),
    ("Cipher", None),
    ("Cipheriv", Some(Class::Cipheriv)),
    ("Decipher", None),
    ("Decipheriv", Some(Class::Decipheriv)),
    ("DiffieHellman", Some(Class::DiffieHellman)),
    ("DiffieHellmanGroup", Some(Class::DiffieHellmanGroup)),
    ("ECDH", Some(Class::Ecdh)),
    ("KeyObject", Some(Class::KeyObject)),
    ("Sign", Some(Class::Sign)),
    ("Verify", Some(Class::Verify)),
    ("X509Certificate", Some(Class::X509Certificate)),
];

/// The class exports Node wraps in `util.deprecate`: `(name, message, code)`.
const DEPRECATED_EXPORTS: [(&str, &str, &str); 2] = [
    ("Hash", "crypto.Hash constructor is deprecated.", "DEP0179"),
    ("Hmac", "crypto.Hmac constructor is deprecated.", "DEP0181"),
];

/// The `crypto` module, built on first use.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    if let Some(intrinsics) = state(c).borrow().crypto.intrinsics {
        return Ok(intrinsics.module);
    }
    let undefined = c.undefined();
    let handle = c.private_symbol_rooted(Some("kHandle"));
    let handle = c.retain(handle)?;
    let key_type = c.symbol_rooted(Some("kKeyType"));
    let key_type = c.retain(key_type)?;
    let module = c.object_rooted()?;
    let module = c.retain(module)?;
    let classes = [undefined; Class::COUNT];
    // Publish the symbol and module first: the classes' builders read them.
    state(c).borrow_mut().crypto.intrinsics = Some(Intrinsics { handle, key_type, classes, module, webcrypto: undefined });
    match build(c, module) {
        Ok(()) => Ok(module),
        Err(error) => {
            state(c).borrow_mut().crypto.intrinsics = None;
            Err(error)
        }
    }
}

fn build(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let classes = build_classes(c, module)?;
    let webcrypto = web::create_crypto(c)?;
    let webcrypto = c.retain(webcrypto)?;
    if let Some(intrinsics) = state(c).borrow_mut().crypto.intrinsics.as_mut() {
        intrinsics.webcrypto = webcrypto;
    }
    for &(name, operation, own_name) in FUNCTIONS {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), own_name)?;
        put(c, module, name, function)?;
    }
    export_classes(c, module, &classes)?;
    install_properties(c, module)
}

/// Every class, retained and published in the intrinsics.
fn build_classes(c: &mut Context<'_>, module: RootId) -> Result<[RootId; Class::COUNT], RootedError> {
    let transform = stream_class(c, "Transform")?;
    let writable = stream_class(c, "Writable")?;
    let mut built: Vec<(Class, RootId)> = Vec::new();
    let lazy = crate::modules::crypto_lazy_transform::constructor(c, transform)?;
    let (hash, hmac) = crate::modules::crypto_hash_source_shared_vm::install(c, module, lazy)?;
    built.extend([(Class::Hash, hash), (Class::Hmac, hmac)]);
    built.extend(cipher::build(c, lazy)?);
    built.extend(sign::build(c, writable)?);
    built.extend(keys::build(c)?);
    built.extend(dh::build(c)?);
    built.extend(x509::build(c)?);
    built.extend(web::build(c)?);
    let mut classes = [c.undefined(); Class::COUNT];
    for (class, constructor) in built {
        classes[class as usize] = c.retain(constructor)?;
    }
    if let Some(intrinsics) = state(c).borrow_mut().crypto.intrinsics.as_mut() {
        intrinsics.classes = classes;
    }
    Ok(classes)
}

/// The module's class exports, deprecated ones behind `util.deprecate`.
fn export_classes(c: &mut Context<'_>, module: RootId, classes: &[RootId; Class::COUNT]) -> Result<(), RootedError> {
    let util = crate::modules::util_shared_vm::module(c)?;
    for &(name, class_id) in CLASS_EXPORTS {
        let mut constructor = match class_id {
            Some(class_id) => classes[class_id as usize],
            None => c.undefined(),
        };
        if let Some((_, message, code)) = DEPRECATED_EXPORTS.iter().find(|(deprecated, ..)| *deprecated == name) {
            let message = c.string_rooted(message);
            let code = c.string_rooted(code);
            constructor = call_method(c, util, "deprecate", &[constructor, message, code])?;
        }
        put(c, module, name, constructor)?;
    }
    Ok(())
}

/// `secureHeapUsed`, the `fips` and `constants` properties, the WebCrypto getters and the
/// `randomBytes` aliases.
fn install_properties(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let secure_heap_used = c.host_function_named(crate::host::shared_vm::operation!("cryptoSecureHeapUsed"), "secureHeapUsed")?;
    put(c, module, "secureHeapUsed", secure_heap_used)?;
    let get_fips = get(c, module, "getFips")?;
    let set_fips = get(c, module, "setFips")?;
    let key = c.string_rooted("fips");
    let fields = DescriptorFields {
        get: Some(get_fips),
        set: Some(set_fips),
        enumerable: Some(false),
        configurable: Some(false),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(module, key, fields)?;
    let constants = crate::modules::crypto_constants::binding(c)?;
    let key = c.string_rooted("constants");
    c.define_data_property_rooted(module, key, constants, false, true, false)?;
    for (name, operation) in [("webcrypto", "cryptoWebcryptoGetter"), ("subtle", "cryptoSubtleGetter"), ("getRandomValues", "cryptoGetRandomValuesGetter")] {
        let getter = c.host_function_named(crate::host::shared_vm::operation(operation), "get")?;
        let key = c.string_rooted(name);
        let fields = DescriptorFields {
            get: Some(getter),
            set: Some(c.undefined()),
            enumerable: Some(true),
            configurable: Some(false),
            ..DescriptorFields::default()
        };
        c.define_property_fields_rooted(module, key, fields)?;
    }
    install_random_bytes_aliases(c, module)
}

/// Accessors reading `randomBytes` that a write replaces with a data property.
fn install_random_bytes_aliases(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let random_bytes = get(c, module, "randomBytes")?;
    for alias in RANDOM_BYTES_ALIASES {
        let name = c.string_rooted(alias);
        let getter = c.host_function_named_with_data(crate::host::shared_vm::operation!("cryptoAliasGet"), "get", random_bytes)?;
        let setter = c.host_function_named_with_data(crate::host::shared_vm::operation!("cryptoAliasSet"), "set", name)?;
        let fields = DescriptorFields {
            get: Some(getter),
            set: Some(setter),
            enumerable: Some(false),
            configurable: Some(true),
            ..DescriptorFields::default()
        };
        c.define_property_fields_rooted(module, name, fields)?;
    }
    Ok(())
}

/// The legacy aliases of `randomBytes`, kept as accessors the ecosystem still reads.
const RANDOM_BYTES_ALIASES: [&str; 3] = ["prng", "pseudoRandomBytes", "rng"];

