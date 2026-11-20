//! OpenSSL provider-backed Argon2 KDF used by Node's Argon2Job.
use super::crypto_kdf::Context;
use super::crypto_secret_material::Material;
use openssl::error::ErrorStack;
use openssl_sys::OSSL_PARAM_construct_uint;

/// Node exposes Argon2Job only when the linked provider has all three modes.
pub(crate) fn available() -> bool {
    [c"ARGON2D", c"ARGON2I", c"ARGON2ID"]
        .into_iter()
        .all(|name| Context::fetch(name).is_ok())
}

pub(crate) fn derive(
    algorithm: u32,
    message: &[u8],
    nonce: &[u8],
    parallelism: u32,
    tag_length: usize,
    memory: u32,
    passes: u32,
    secret: &[u8],
    associated_data: &[u8],
) -> Result<Material, ErrorStack> {
    let name = match algorithm {
        0 => c"ARGON2D",
        1 => c"ARGON2I",
        2 => c"ARGON2ID",
        _ => return Err(ErrorStack::get()),
    };
    if tag_length == 0 {
        return Err(ErrorStack::get());
    }
    let context = Context::fetch(name)?;
    let mut iterations = passes;
    let mut lanes = parallelism;
    // Node computes serially by default; lanes remain an Argon2 input, while
    // provider worker threads are an optional optimization.
    let mut threads: u32 = if parallelism == 0 { 0 } else { 1 };
    let mut memory_cost = memory;
    let mut output = Material::new(vec![0; tag_length]);
    let params = unsafe {
        [
            openssl_sys::OSSL_PARAM_construct_octet_string(
                c"pass".as_ptr(),
                message.as_ptr().cast_mut().cast(),
                message.len(),
            ),
            openssl_sys::OSSL_PARAM_construct_octet_string(
                c"salt".as_ptr(),
                nonce.as_ptr().cast_mut().cast(),
                nonce.len(),
            ),
            openssl_sys::OSSL_PARAM_construct_octet_string(
                c"secret".as_ptr(),
                secret.as_ptr().cast_mut().cast(),
                secret.len(),
            ),
            openssl_sys::OSSL_PARAM_construct_octet_string(
                c"ad".as_ptr(),
                associated_data.as_ptr().cast_mut().cast(),
                associated_data.len(),
            ),
            OSSL_PARAM_construct_uint(c"iter".as_ptr(), &mut iterations),
            OSSL_PARAM_construct_uint(c"lanes".as_ptr(), &mut lanes),
            OSSL_PARAM_construct_uint(c"threads".as_ptr(), &mut threads),
            OSSL_PARAM_construct_uint(c"memcost".as_ptr(), &mut memory_cost),
            openssl_sys::OSSL_PARAM_construct_end(),
        ]
    };
    if unsafe {
        openssl_sys::EVP_KDF_derive(
            context.as_ptr(),
            output.as_mut_ptr(),
            tag_length,
            params.as_ptr(),
        )
    } != 1
    {
        return Err(ErrorStack::get());
    }
    Ok(output)
}
