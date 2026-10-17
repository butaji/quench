//! Project the configured public crypto factory declarations and export
//! expressions from their original AST. Their implementation is never copied.
use crate::host::{NodeHost, shared_vm::commonjs};
use quench_runtime::{NativeContext, RootId, RootedError};
fn get(
    c: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
pub(crate) fn install(
    c: &mut NativeContext<'_, NodeHost>,
    public: RootId,
) -> Result<(), RootedError> {
    c.with_startup_intrinsics(|c| {
        let root = c.host_mut().node_library_root.clone();
        let filename = root.join("lib/crypto.js");
        let source = std::fs::read_to_string(&filename)
            .or_else(|error| {
                crate::node_sources::source(&filename, &root)
                    .map(str::to_owned)
                    .ok_or(error)
            })
            .map_err(|error| RootedError::host(error.to_string()))?;
        let factory = super::crypto_hash_source::factory(&source).map_err(RootedError::host)?;
        let hash_module = commonjs::node_library_module(c, "crypto.js", "internal/crypto/hash.js")?;
        let hash = get(c, hash_module, "Hash")?;
        let hmac = get(c, hash_module, "Hmac")?;
        let deprecate =
            commonjs::node_library_export(c, "crypto.js", "internal/util.js", "deprecate")?;
        let factory = c.evaluate_script_rooted(&factory, &filename.to_string_lossy())?;
        let undefined = c.undefined();
        let projected = c.call_rooted(factory, undefined, &[hash, hmac, deprecate])?;
        for name in ["Hash", "Hmac", "createHash", "createHmac", "hash"] {
            let owner = if name == "hash" {
                hash_module
            } else {
                projected
            };
            let value = get(c, owner, name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(public, key, value, true, true, true)?;
        }
        let random = commonjs::node_library_module(c, "crypto.js", "internal/crypto/random.js")?;
        for name in ["randomBytes", "randomFill", "randomFillSync", "randomInt", "randomUUID", "randomUUIDv7"] {
            let value = get(c, random, name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(public, key, value, true, true, true)?;
        }
        let factory = super::crypto_hash_source::random_alias_factory(&source).map_err(RootedError::host)?;
        let factory = c.evaluate_script_rooted(&factory, &filename.to_string_lossy())?;
        let random_bytes = get(c, random, "randomBytes")?;
        let options = commonjs::node_library_export(c, "crypto.js", "internal/options.js", "getOptionValue")?;
        let global = c.global_root()?;
        let object = get(c, global, "Object")?;
        let define = get(c, object, "defineProperty")?;
        let define_properties = get(c, object, "defineProperties")?;
        let aliases = c.call_rooted(factory, undefined, &[random_bytes, deprecate, options, define])?;
        c.call_rooted(define_properties, object, &[public, aliases])?;
        let hashes =
            commonjs::node_library_export(c, "crypto.js", "internal/crypto/util.js", "getHashes")?;
        let key = c.string_rooted("getHashes");
        c.define_data_property_rooted(public, key, hashes, true, true, true)?;
        let keys = commonjs::node_library_module(c, "crypto.js", "internal/crypto/keys.js")?;
        for name in [
            "KeyObject",
            "createSecretKey",
            "createPublicKey",
            "createPrivateKey",
        ] {
            let value = get(c, keys, name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(public, key, value, true, true, true)?;
        }
        let sig = commonjs::node_library_module(c, "crypto.js", "internal/crypto/sig.js")?;
        let sign = get(c, sig, "Sign")?;
        let verify = get(c, sig, "Verify")?;
        let factory =
            super::crypto_hash_source::signature_factory(&source).map_err(RootedError::host)?;
        let factory = c.evaluate_script_rooted(&factory, &filename.to_string_lossy())?;
        let projected = c.call_rooted(factory, undefined, &[sign, verify, deprecate])?;
        for name in ["Sign", "Verify", "createSign", "createVerify"] {
            let value = get(c, projected, name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(public, key, value, true, true, true)?;
        }
        for (name, export) in [("sign", "signOneShot"), ("verify", "verifyOneShot")] {
            let value = get(c, sig, export)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(public, key, value, true, true, true)?;
        }
        for (module, names) in [
            (
                "internal/crypto/keygen.js",
                &["generateKeyPair", "generateKeyPairSync"][..],
            ),
            (
                "internal/crypto/cipher.js",
                &[
                    "publicEncrypt",
                    "publicDecrypt",
                    "privateEncrypt",
                    "privateDecrypt",
                ][..],
            ),
        ] {
            let owner = commonjs::node_library_module(c, "crypto.js", module)?;
            for name in names {
                let value = get(c, owner, name)?;
                let key = c.string_rooted(name);
                c.define_data_property_rooted(public, key, value, true, true, true)?;
            }
        }
        let hkdf = commonjs::node_library_module(c, "crypto.js", "internal/crypto/hkdf.js")?;
        for name in ["hkdf", "hkdfSync"] {
            let value = get(c, hkdf, name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(public, key, value, true, true, true)?;
        }
        let pbkdf2 = commonjs::node_library_module(c, "crypto.js", "internal/crypto/pbkdf2.js")?;
        for name in ["pbkdf2", "pbkdf2Sync"] {
            let value = get(c, pbkdf2, name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(public, key, value, true, true, true)?;
        }
        Ok(())
    })
}
