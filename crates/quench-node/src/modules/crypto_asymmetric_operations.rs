//! Native consumers borrow canonical PKey owners directly, never serialize or
//! reinterpret KeyObjects as byte input. SDK modules retain argument policy.
use super::{
    crypto_asymmetric::{KeyError, OwnedKey},
    crypto_digest::Digest,
};
use openssl::{
    bn::BigNum, dsa::DsaSig, ecdsa::EcdsaSig, md_ctx::MdCtx, pkey_ctx::PkeyCtx, rsa::Padding,
};
pub(crate) struct SignatureOptions {
    pub padding: Option<Padding>,
    pub salt_length: Option<i32>,
    pub ieee_p1363: bool,
}
pub(crate) enum RsaOperation {
    PublicEncrypt,
    PrivateDecrypt,
    PrivateEncrypt,
    PublicDecrypt,
}
pub(crate) struct RsaOptions<'a> {
    pub padding: Padding,
    pub digest: Option<&'a Digest>,
    pub mgf1_digest: Option<&'a Digest>,
    pub label: Option<&'a [u8]>,
}
pub(crate) fn apply_signature_options<T>(
    context: &mut openssl::pkey_ctx::PkeyCtxRef<T>,
    id: openssl::pkey::Id,
    options: &SignatureOptions,
) -> Result<(), openssl::error::ErrorStack> {
    if matches!(id, openssl::pkey::Id::RSA | openssl::pkey::Id::RSA_PSS) {
        if let Some(padding) = options.padding {
            // RSA-PSS keys are already constrained to PSS by OpenSSL. Some
            // providers reject an explicit RSA padding control on that key
            // type, even when it repeats the only supported mode. WebCrypto
            // always supplies RSA_PKCS1_PSS_PADDING, so leave the key's native
            // restriction in place instead of applying the redundant control.
            if id != openssl::pkey::Id::RSA_PSS || padding != Padding::PKCS1_PSS {
                context.set_rsa_padding(padding)?;
            }
        }
        if let Some(length) = options.salt_length {
            context.set_rsa_pss_saltlen(openssl::sign::RsaPssSaltlen::custom(length))?;
        }
    }
    Ok(())
}
pub(crate) fn sign(
    key: &OwnedKey,
    digest: &Digest,
    input: &[u8],
    options: &SignatureOptions,
) -> Result<Vec<u8>, KeyError> {
    let key = key
        .private_ref()
        .ok_or(KeyError::Unsupported("private key required"))?;
    let mut context = MdCtx::new()?;
    let pkey_context = context.digest_sign_init(Some(digest.native_md()), key)?;
    apply_signature_options(pkey_context, key.id(), options)?;
    context.digest_sign_update(input)?;
    let mut output = Vec::new();
    context.digest_sign_final_to_vec(&mut output)?;
    if options.ieee_p1363 {
        to_p1363(key, &output)
    } else {
        Ok(output)
    }
}
pub(crate) fn verify(
    key: &OwnedKey,
    digest: &Digest,
    input: &[u8],
    signature: &[u8],
    options: &SignatureOptions,
) -> Result<bool, KeyError> {
    let mut context = MdCtx::new()?;
    let pkey_context = context.digest_verify_init(Some(digest.native_md()), key.public_ref())?;
    apply_signature_options(pkey_context, key.public_ref().id(), options)?;
    let converted;
    let signature = if options.ieee_p1363 {
        match from_p1363(key.public_ref(), signature)? {
            Some(value) => {
                converted = value;
                &converted
            }
            None => return Ok(false),
        }
    } else {
        signature
    };
    context.digest_verify_update(input)?;
    Ok(context.digest_verify_final(signature).unwrap_or(false))
}
pub(crate) fn rsa(
    key: &OwnedKey,
    operation: RsaOperation,
    input: &[u8],
    options: &RsaOptions<'_>,
) -> Result<Vec<u8>, KeyError> {
    // Public and private contexts each retain their typed native PKey borrow until
    // completion. OAEP controls use the actual owned Md, including provider-only IDs.
    macro_rules! configure {
        ($context:expr) => {{
            let context = &mut $context;
            context.set_rsa_padding(options.padding)?;
            if let Some(digest) = options.digest {
                context.set_rsa_oaep_md(digest.native_md())?
            }
            if let Some(digest) = options.mgf1_digest {
                context.set_rsa_mgf1_md(digest.native_md())?
            }
            if let Some(label) = options.label {
                context.set_rsa_oaep_label(label)?
            }
        }};
    }
    let mut output = Vec::new();
    match operation {
        RsaOperation::PublicEncrypt => {
            let mut context = PkeyCtx::new(key.public_ref())?;
            context.encrypt_init()?;
            configure!(context);
            context.encrypt_to_vec(input, &mut output)?;
        }
        RsaOperation::PrivateDecrypt => {
            let key = key
                .private_ref()
                .ok_or(KeyError::Unsupported("private key required"))?;
            let mut context = PkeyCtx::new(key)?;
            context.decrypt_init()?;
            configure!(context);
            context.decrypt_to_vec(input, &mut output)?;
        }
        RsaOperation::PrivateEncrypt => {
            let key = key
                .private_ref()
                .ok_or(KeyError::Unsupported("private key required"))?;
            let mut context = PkeyCtx::new(key)?;
            context.sign_init()?;
            configure!(context);
            context.sign_to_vec(input, &mut output)?;
        }
        RsaOperation::PublicDecrypt => {
            let mut context = PkeyCtx::new(key.public_ref())?;
            context.verify_recover_init()?;
            configure!(context);
            let size = context.verify_recover(input, None)?;
            output.resize(size, 0);
            let size = context.verify_recover(input, Some(&mut output))?;
            output.truncate(size);
        }
    }
    Ok(output)
}

fn signature_width<T: openssl::pkey::HasPublic>(
    key: &openssl::pkey::PKeyRef<T>,
) -> Result<Option<usize>, KeyError> {
    Ok(match key.id() {
        openssl::pkey::Id::EC => Some(key.ec_key()?.group().degree().div_ceil(8) as usize),
        openssl::pkey::Id::DSA => Some((key.dsa()?.q().num_bits() as usize).div_ceil(8)),
        _ => None,
    })
}
pub(crate) fn to_p1363<T: openssl::pkey::HasPublic>(
    key: &openssl::pkey::PKeyRef<T>,
    signature: &[u8],
) -> Result<Vec<u8>, KeyError> {
    let Some(width) = signature_width(key)? else {
        return Ok(signature.to_vec());
    };
    let (r, s) = if key.id() == openssl::pkey::Id::DSA {
        let sig = DsaSig::from_der(signature)?;
        (
            sig.r().to_vec_padded(width as i32)?,
            sig.s().to_vec_padded(width as i32)?,
        )
    } else {
        let sig = EcdsaSig::from_der(signature)?;
        (
            sig.r().to_vec_padded(width as i32)?,
            sig.s().to_vec_padded(width as i32)?,
        )
    };
    let mut output = r;
    output.extend(s);
    Ok(output)
}
pub(crate) fn from_p1363<T: openssl::pkey::HasPublic>(
    key: &openssl::pkey::PKeyRef<T>,
    signature: &[u8],
) -> Result<Option<Vec<u8>>, KeyError> {
    let Some(width) = signature_width(key)? else {
        return Ok(Some(signature.to_vec()));
    };
    if signature.len() != width * 2 {
        return Ok(None);
    }
    let r = BigNum::from_slice(&signature[..width])?;
    let s = BigNum::from_slice(&signature[width..])?;
    Ok(Some(if key.id() == openssl::pkey::Id::DSA {
        DsaSig::from_private_components(r, s)?.to_der()?
    } else {
        EcdsaSig::from_private_components(r, s)?.to_der()?
    }))
}
/// Streaming Sign/Verify hashes before the key is supplied. These operations
/// borrow the exact native digest that initialized that context.
pub(crate) fn sign_prehashed(
    key: &OwnedKey,
    digest: &Digest,
    input: &[u8],
    options: &SignatureOptions,
) -> Result<Vec<u8>, KeyError> {
    let private = key
        .private_ref()
        .ok_or(KeyError::Unsupported("private key required"))?;
    let mut context = PkeyCtx::new(private)?;
    context.sign_init()?;
    apply_signature_options(&mut context, private.id(), options)?;
    context.set_signature_md(digest.native_md())?;
    let mut output = Vec::new();
    context.sign_to_vec(input, &mut output)?;
    if options.ieee_p1363 {
        to_p1363(private, &output)
    } else {
        Ok(output)
    }
}
pub(crate) fn verify_prehashed(
    key: &OwnedKey,
    digest: &Digest,
    input: &[u8],
    signature: &[u8],
    options: &SignatureOptions,
) -> Result<bool, KeyError> {
    let mut context = PkeyCtx::new(key.public_ref())?;
    context.verify_init()?;
    if apply_signature_options(&mut context, key.public_ref().id(), options).is_err() {
        return Ok(false);
    }
    if context.set_signature_md(digest.native_md()).is_err() {
        return Ok(false);
    }
    let converted;
    let signature = if options.ieee_p1363 {
        match from_p1363(key.public_ref(), signature)? {
            Some(value) => {
                converted = value;
                &converted
            }
            None => return Err(KeyError::Unsupported("Malformed signature")),
        }
    } else {
        signature
    };
    Ok(context.verify(input, signature).unwrap_or(false))
}
