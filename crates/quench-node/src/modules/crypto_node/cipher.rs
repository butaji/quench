//! `Cipheriv` and `Decipheriv`, `getCiphers()` and `getCipherInfo()`.

use super::errors::{self, CryptoError};
use super::{arg, attach, construct_if_called, define_methods, get, put, with_native, Class, Context, Members, Native, Outcome};
use crate::modules::string_decoder_codec::{self as decoder, DecoderState, Layout};
use foreign_types::ForeignTypeRef;
use openssl::cipher::{Cipher, CipherRef};
use openssl::cipher_ctx::CipherCtx;
use quench_runtime::{RootId, RootedError};

/// `EVP_CIPH_*_MODE` and the AEAD flag, from openssl/evp.h.
mod mode {
    pub(super) const STREAM: u64 = 0x0;
    pub(super) const ECB: u64 = 0x1;
    pub(super) const CBC: u64 = 0x2;
    pub(super) const CFB: u64 = 0x3;
    pub(super) const OFB: u64 = 0x4;
    pub(super) const CTR: u64 = 0x5;
    pub(super) const GCM: u64 = 0x6;
    pub(super) const CCM: u64 = 0x7;
    pub(super) const XTS: u64 = 0x10001;
    pub(super) const WRAP: u64 = 0x10002;
    pub(super) const OCB: u64 = 0x10003;
    pub(super) const MASK: u64 = 0xF0007;
    pub(super) const AEAD_FLAG: u64 = 0x20_0000;
}

/// The largest authentication tag, and GCM's valid tag lengths (NIST SP 800-38D).
const MAX_TAG_LENGTH: usize = 16;
const GCM_SHORT_TAG_LENGTHS: [usize; 2] = [4, 8];
const GCM_MIN_LONG_TAG_LENGTH: usize = 12;
/// ChaCha20-Poly1305's tag length and largest nonce.
const CHACHA20_POLY1305_TAG_LENGTH: usize = 16;
const CHACHA20_POLY1305_MAX_IV_LENGTH: usize = 12;
/// CCM's message bound for 12- and 13-byte nonces: 2^(8 * (15 - nonce length)) - 1.
const CCM_NONCE_MESSAGE_LIMITS: [(usize, usize); 2] = [(12, 0xFF_FFFF), (13, 0xFFFF)];

fn is_valid_gcm_tag_length(length: usize) -> bool {
    GCM_SHORT_TAG_LENGTHS.contains(&length) || (GCM_MIN_LONG_TAG_LENGTH..=MAX_TAG_LENGTH).contains(&length)
}

/// The cipher a name (or alias) fetches.
pub(crate) fn cipher_by_name(name: &str) -> Option<Cipher> {
    let _errors = crate::modules::crypto_provider_facts::ErrorScope::new();
    Cipher::fetch(None, name.split('\0').next().unwrap_or_default(), None).ok()
}

fn cipher_mode(cipher: &CipherRef) -> u64 {
    // SAFETY: the cipher is a live EVP_CIPHER.
    let flags = unsafe { openssl_sys::EVP_CIPHER_get_flags(cipher.as_ptr()) };
    flags & mode::MASK
}

fn is_aead(cipher: &CipherRef) -> bool {
    // SAFETY: the cipher is a live EVP_CIPHER.
    let flags = unsafe { openssl_sys::EVP_CIPHER_get_flags(cipher.as_ptr()) };
    matches!(cipher_mode(cipher), mode::GCM | mode::CCM | mode::OCB) || flags & mode::AEAD_FLAG != 0
}

fn is_chacha20_poly1305(cipher: &CipherRef) -> bool {
    cipher.nid() == openssl::nid::Nid::CHACHA20_POLY1305
}

/// Where an AEAD decipher's expected tag is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagState {
    Unknown,
    Known,
    PassedToOpenSsl,
}

/// One cipher or decipher operation.
pub(crate) struct CipherState {
    /// The context, until `final()`.
    context: Option<CipherCtx>,
    decrypt: bool,
    mode: u64,
    aead: bool,
    tag_length: Option<usize>,
    tag: [u8; MAX_TAG_LENGTH],
    tag_state: TagState,
    max_message_size: usize,
    pending_auth_failed: bool,
    /// The output decoder `update`/`final` share once given an encoding.
    decoder: Option<DecoderState>,
}

/// What failed in `update` (Node's `kErrorState`/`kErrorMessageSize`).
enum UpdateError {
    State,
    MessageSize,
    OpenSsl(openssl::error::ErrorStack),
}

impl CipherState {
    fn pass_tag(&mut self) -> bool {
        if self.tag_state != TagState::Known {
            return true;
        }
        let length = self.tag_length.unwrap_or(MAX_TAG_LENGTH);
        let Some(context) = self.context.as_mut() else {
            return false;
        };
        let passed = context.set_tag(&self.tag[..length]).is_ok();
        self.tag_state = TagState::PassedToOpenSsl;
        passed
    }

    fn update(&mut self, input: &[u8]) -> Result<Vec<u8>, UpdateError> {
        if self.context.is_none() {
            return Err(UpdateError::State);
        }
        if self.mode == mode::CCM && input.len() > self.max_message_size {
            return Err(UpdateError::MessageSize);
        }
        if self.decrypt && self.aead {
            self.pass_tag();
        }
        let context = self.context.as_mut().ok_or(UpdateError::State)?;
        let mut output = Vec::new();
        match context.cipher_update_vec(input, &mut output) {
            Ok(_) => Ok(output),
            Err(_) if self.decrypt && self.mode == mode::CCM => {
                self.pending_auth_failed = true;
                Ok(output)
            }
            Err(stack) => Err(UpdateError::OpenSsl(stack)),
        }
    }

    /// `final()`: the remaining output, or the OpenSSL errors of the failure (possibly none).
    fn finish(&mut self) -> Result<Vec<u8>, openssl::error::ErrorStack> {
        if self.decrypt && self.aead {
            self.pass_tag();
        }
        let mut context = self.context.take().ok_or_else(openssl::error::ErrorStack::get)?;
        let mut output = Vec::new();
        if self.decrypt && self.mode == mode::CCM {
            return if self.pending_auth_failed { Err(openssl::error::ErrorStack::get()) } else { Ok(output) };
        }
        context.cipher_final_vec(&mut output)?;
        if !self.decrypt && self.aead {
            let length = *self.tag_length.get_or_insert(MAX_TAG_LENGTH);
            context.tag(&mut self.tag[..length])?;
        }
        Ok(output)
    }
}

/// `options[name]`, which must be an unsigned 32-bit integer when present.
fn uint_option(c: &mut Context<'_>, options: RootId, name: &str) -> Result<Option<usize>, RootedError> {
    let value = super::option(c, options, name)?;
    if super::is_undefined(c, value) {
        return Ok(None);
    }
    let valid = c.rooted_value(value).and_then(|value| value.as_number()).filter(|number| {
        number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(number)
    });
    match valid {
        Some(number) => Ok(Some(number as usize)),
        None => Err(errors::invalid_value(c, value, &format!("options.{name}"), "is invalid")),
    }
}

fn auth_tag_error(c: &mut Context<'_>, message: String) -> RootedError {
    errors::throw(c, CryptoError::InvalidAuthTag(message))
}

/// Node's `CipherBase::InitIv`: fetch the cipher, check the key, nonce and tag lengths.
fn initialize(c: &mut Context<'_>, name: &str, key: &[u8], iv: Option<&[u8]>, tag_length: Option<usize>, decrypt: bool) -> Result<CipherState, RootedError> {
    let Some(cipher) = cipher_by_name(name) else {
        return Err(errors::throw(c, CryptoError::UnknownCipher));
    };
    let expected_iv = cipher.iv_length();
    let aead = is_aead(&cipher);
    let iv = iv.unwrap_or_default();
    let has_iv = !iv.is_empty();
    let mode = cipher_mode(&cipher);
    if (!has_iv && expected_iv != 0) || (!aead && has_iv && iv.len() != expected_iv) {
        return Err(errors::throw(c, CryptoError::InvalidIv));
    }
    if is_chacha20_poly1305(&cipher) && iv.len() > CHACHA20_POLY1305_MAX_IV_LENGTH {
        return Err(errors::throw(c, CryptoError::InvalidIv));
    }
    let openssl_error = |c: &mut Context<'_>, stack| errors::openssl(c, stack);
    let mut context = CipherCtx::new().map_err(|stack| openssl_error(c, stack))?;
    let initialized = if decrypt { context.decrypt_init(Some(&cipher), None, None) } else { context.encrypt_init(Some(&cipher), None, None) };
    initialized.map_err(|stack| openssl_error(c, stack))?;
    if mode == mode::WRAP {
        // SAFETY: the context is live; wrap modes must be allowed before use.
        unsafe { openssl_sys::EVP_CIPHER_CTX_set_flags(context.as_ptr(), openssl_sys::EVP_CIPHER_CTX_FLAG_WRAP_ALLOW) };
    }
    if key.len() != cipher.key_length() && context.set_key_length(key.len()).is_err() {
        return Err(errors::throw(c, CryptoError::InvalidKeyLength));
    }
    let mut state = CipherState {
        context: None,
        decrypt,
        mode,
        aead,
        tag_length: None,
        tag: [0; MAX_TAG_LENGTH],
        tag_state: TagState::Unknown,
        max_message_size: i32::MAX as usize,
        pending_auth_failed: false,
        decoder: None,
    };
    if aead {
        if context.set_iv_length(iv.len()).is_err() {
            return Err(errors::throw(c, CryptoError::InvalidIv));
        }
        if mode == mode::GCM {
            if let Some(length) = tag_length {
                if !is_valid_gcm_tag_length(length) {
                    return Err(auth_tag_error(c, format!("Invalid authentication tag length: {length}")));
                }
                state.tag_length = Some(length);
            }
        } else {
            let length = match tag_length {
                Some(length) => length,
                None if is_chacha20_poly1305(&cipher) => CHACHA20_POLY1305_TAG_LENGTH,
                None => return Err(auth_tag_error(c, format!("authTagLength required for {name}"))),
            };
            if context.set_tag_length(length).is_err() {
                return Err(auth_tag_error(c, format!("Invalid authentication tag length: {length}")));
            }
            state.tag_length = Some(length);
            if mode == mode::CCM {
                for (nonce, limit) in CCM_NONCE_MESSAGE_LIMITS {
                    if iv.len() == nonce {
                        state.max_message_size = limit;
                    }
                }
            }
        }
    }
    let keyed = if decrypt { context.decrypt_init(None, Some(key), Some(iv)) } else { context.encrypt_init(None, Some(key), Some(iv)) };
    if keyed.is_err() {
        return Err(errors::throw(c, CryptoError::InvalidKeyLength));
    }
    state.context = Some(context);
    Ok(state)
}

fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId], decrypt: bool) -> Outcome {
    let class = if decrypt { Class::Decipheriv } else { Class::Cipheriv };
    if let Some(instance) = construct_if_called(c, this, class, args)? {
        return Ok(instance);
    }
    let (name, key, iv, options) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let name = super::string_argument(c, name, "cipher")?;
    let encoding = super::hash::string_option(c, options, "encoding")?;
    let key = super::keys::prepare_secret_key(c, key, encoding)?;
    let iv = if super::is_null(c, iv) {
        None
    } else {
        let undefined = c.undefined();
        Some(super::binary_like(c, iv, "iv", undefined)?)
    };
    let tag_length = uint_option(c, options, "authTagLength")?;
    let state = initialize(c, &name, &key, iv.as_deref(), tag_length, decrypt)?;
    attach(c, this, Native::Cipher(Box::new(state)))?;
    let null = c.null();
    put(c, this, "_decoder", null)?;
    put(c, this, "_options", options)?;
    Ok(c.undefined())
}

/// `new Cipheriv(algorithm, key, iv[, options])`.
pub(crate) fn cipheriv(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    construct(c, this, args, false)
}

/// `new Decipheriv(algorithm, key, iv[, options])`.
pub(crate) fn decipheriv(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    construct(c, this, args, true)
}

fn state<R>(c: &mut Context<'_>, this: RootId, f: impl FnOnce(&mut CipherState) -> R) -> Result<R, RootedError> {
    let result = with_native(c, this, |native| match native {
        Native::Cipher(state) => Some(f(state)),
        _ => None,
    })?;
    result.flatten().ok_or_else(|| errors::throw(c, CryptoError::PlainType("Illegal invocation")))
}

/// The output of `update`/`final`: a Buffer, or text through the shared decoder.
fn output(c: &mut Context<'_>, this: RootId, bytes: Vec<u8>, encoding: RootId, last: bool) -> Outcome {
    let name = match c.string_text(encoding)? {
        Some(name) if c.truthy_rooted(encoding)? && name != "buffer" => name,
        _ => return super::buffer(c, bytes),
    };
    let Some(index) = super::canonical_encoding(&name).and_then(|canonical| decoder::ENCODINGS.iter().position(|known| *known == canonical)) else {
        return Err(errors::throw(c, CryptoError::UnknownEncoding(name)));
    };
    let units = state(c, this, |state| {
        let decoder = state.decoder.get_or_insert_with(|| {
            let mut layout = [0u8; Layout::SIZE];
            layout[Layout::ENCODING] = index as u8;
            DecoderState(layout)
        });
        let mut units = decoder.decode(&bytes);
        if last {
            if let Ok(units) = units.as_mut() {
                units.extend(decoder.flush().unwrap_or_default());
            }
        }
        units
    })?;
    match units {
        Ok(units) => Ok(c.string_units_rooted(&units)),
        Err(_) => Err(errors::throw(c, CryptoError::Plain("Cannot create a string longer than the maximum string length"))),
    }
}

fn update_bytes(c: &mut Context<'_>, this: RootId, bytes: &[u8]) -> Result<Vec<u8>, RootedError> {
    match state(c, this, |state| state.update(bytes))? {
        Ok(output) => Ok(output),
        Err(UpdateError::State) => Err(errors::throw(c, CryptoError::Plain("Trying to add data in unsupported state"))),
        Err(UpdateError::MessageSize) => Err(errors::throw(c, CryptoError::InvalidMessageLength)),
        Err(UpdateError::OpenSsl(stack)) => Err(errors::openssl(c, stack)),
    }
}

/// `cipher.update(data[, inputEncoding][, outputEncoding])`.
pub(crate) fn update(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (data, input_encoding, output_encoding) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let bytes = super::update_data(c, data, input_encoding)?;
    let result = update_bytes(c, this, &bytes)?;
    output(c, this, result, output_encoding, false)
}

fn final_bytes(c: &mut Context<'_>, this: RootId) -> Result<Vec<u8>, RootedError> {
    let (started, aead) = state(c, this, |state| (state.context.is_some(), state.aead))?;
    if !started {
        return Err(errors::throw(c, CryptoError::InvalidState(None)));
    }
    match state(c, this, CipherState::finish)? {
        Ok(output) => Ok(output),
        Err(stack) => {
            if !stack.errors().is_empty() {
                return Err(errors::openssl(c, stack));
            }
            let message = if aead { "Unsupported state or unable to authenticate data" } else { "Unsupported state" };
            Err(errors::throw(c, CryptoError::Plain(message)))
        }
    }
}

/// `cipher.final([outputEncoding])`.
pub(crate) fn final_(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    let bytes = final_bytes(c, this)?;
    output(c, this, bytes, encoding, true)
}

/// `cipher.setAutoPadding([autoPadding = true])`.
pub(crate) fn set_auto_padding(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let padding = arg(c, args, 0);
    let padding = super::is_undefined(c, padding) || c.truthy_rooted(padding)?;
    let applied = state(c, this, |state| match state.context.as_mut() {
        Some(context) => {
            context.set_padding(padding);
            true
        }
        None => false,
    })?;
    if !applied {
        return Err(errors::throw(c, CryptoError::InvalidState(Some("setAutoPadding"))));
    }
    Ok(this)
}

/// `cipher.getAuthTag()`: after `final()` of an AEAD encryption.
pub(crate) fn get_auth_tag(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let tag = state(c, this, |state| {
        let finished = state.context.is_none() && !state.decrypt;
        state.tag_length.filter(|length| finished && *length != 0).map(|length| state.tag[..length].to_vec())
    })?;
    match tag {
        Some(tag) => super::buffer(c, tag),
        None => Err(errors::throw(c, CryptoError::InvalidState(Some("getAuthTag")))),
    }
}

/// `decipher.setAuthTag(buffer[, encoding])`.
pub(crate) fn set_auth_tag(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (tag, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let tag = super::binary_like(c, tag, "buffer", encoding)?;
    let ready = state(c, this, |state| state.context.is_some() && state.aead && state.decrypt && state.tag_state == TagState::Unknown)?;
    if !ready {
        return Err(errors::throw(c, CryptoError::InvalidState(Some("setAuthTag"))));
    }
    let valid = state(c, this, |state| {
        if state.mode == mode::GCM {
            state.tag_length.is_none_or(|length| length == tag.len()) && is_valid_gcm_tag_length(tag.len())
        } else {
            state.tag_length == Some(tag.len())
        }
    })?;
    if !valid {
        return Err(auth_tag_error(c, format!("Invalid authentication tag length: {}", tag.len())));
    }
    state(c, this, |state| {
        state.tag_length = Some(tag.len());
        state.tag_state = TagState::Known;
        state.tag = [0; MAX_TAG_LENGTH];
        state.tag[..tag.len()].copy_from_slice(&tag);
    })?;
    Ok(this)
}

/// `cipher.setAAD(buffer[, options])`.
pub(crate) fn set_aad(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (aad, options) = (arg(c, args, 0), arg(c, args, 1));
    let encoding = super::hash::string_option(c, options, "encoding")?;
    let plaintext_length = uint_option(c, options, "plaintextLength")?;
    let aad = super::binary_like(c, aad, "aadbuf", encoding)?;
    let ccm = state(c, this, |state| state.mode == mode::CCM)?;
    if ccm && plaintext_length.is_none() {
        return Err(errors::throw(c, CryptoError::MissingArgs("options.plaintextLength required for CCM mode with AAD")));
    }
    let applied = state(c, this, |state| {
        if state.context.is_none() || !state.aead {
            return false;
        }
        if state.mode == mode::CCM {
            let length = plaintext_length.unwrap_or_default();
            if length > state.max_message_size || (state.decrypt && !state.pass_tag()) {
                return false;
            }
            if state.context.as_mut().is_none_or(|context| context.set_data_len(length).is_err()) {
                return false;
            }
        }
        state.context.as_mut().is_some_and(|context| context.cipher_update(&aad, None).is_ok())
    })?;
    if !applied {
        return Err(errors::throw(c, CryptoError::InvalidState(Some("setAAD"))));
    }
    Ok(this)
}

/// `_transform(chunk, encoding, callback)`: push the chunk's output.
pub(crate) fn transform(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, encoding, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let bytes = super::update_data(c, chunk, encoding)?;
    let output = update_bytes(c, this, &bytes)?;
    let output = super::buffer(c, output)?;
    super::call_method(c, this, "push", &[output])?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])?;
    Ok(undefined)
}

/// `_flush(callback)`: push the final output, or fail the stream.
pub(crate) fn flush(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    match super::attempt(final_bytes(c, this))? {
        Ok(bytes) => {
            let bytes = super::buffer(c, bytes)?;
            super::call_method(c, this, "push", &[bytes])?;
            c.call_rooted(callback, undefined, &[])?;
        }
        Err(error) => {
            c.call_rooted(callback, undefined, &[error])?;
        }
    }
    Ok(undefined)
}

/// The methods both classes share, and each one's own.
const SHARED_METHODS: [(&str, &str); 5] = [
    ("_transform", "cryptoCipherTransform"),
    ("_flush", "cryptoCipherFlush"),
    ("update", "cryptoCipherUpdate"),
    ("final", "cryptoCipherFinal"),
    ("setAutoPadding", "cryptoCipherSetAutoPadding"),
];

pub(crate) fn build(c: &mut Context<'_>, lazy: RootId) -> Result<Vec<(Class, RootId)>, RootedError> {
    let mut classes = Vec::new();
    for (class, operation, name, own) in [
        (Class::Cipheriv, "cryptoCipheriv", "Cipheriv", ("getAuthTag", "cryptoCipherGetAuthTag")),
        (Class::Decipheriv, "cryptoDecipheriv", "Decipheriv", ("setAuthTag", "cryptoCipherSetAuthTag")),
    ] {
        let constructor = super::subclass(c, operation, name, lazy)?;
        let prototype = get(c, constructor, "prototype")?;
        define_methods(c, prototype, &SHARED_METHODS, Members::Assigned)?;
        define_methods(c, prototype, &[own, ("setAAD", "cryptoCipherSetAad")], Members::Assigned)?;
        classes.push((class, constructor));
    }
    Ok(classes)
}

/// `crypto.createCipheriv(algorithm, key, iv[, options])`.
pub(crate) fn create_cipheriv(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::Cipheriv)?;
    let values: Vec<RootId> = (0..4).map(|index| arg(c, args, index)).collect();
    c.construct_rooted(constructor, constructor, &values)
}

/// `crypto.createDecipheriv(algorithm, key, iv[, options])`.
pub(crate) fn create_decipheriv(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::Decipheriv)?;
    let values: Vec<RootId> = (0..4).map(|index| arg(c, args, index)).collect();
    c.construct_rooted(constructor, constructor, &values)
}

/// `crypto.getCiphers()`: every cipher name the provider fetches, lowercase and sorted.
pub(crate) fn get_ciphers(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let names = super::registered_names(super::Registry::Ciphers, |name| cipher_by_name(name).is_some());
    super::string_array(c, &names)
}

fn mode_name(mode: u64) -> &'static str {
    match mode {
        mode::ECB => "ecb",
        mode::CBC => "cbc",
        mode::CFB => "cfb",
        mode::OFB => "ofb",
        mode::CTR => "ctr",
        mode::GCM => "gcm",
        mode::CCM => "ccm",
        mode::XTS => "xts",
        mode::WRAP => "wrap",
        mode::OCB => "ocb",
        mode::STREAM => "stream",
        _ => "unknown",
    }
}

/// `crypto.getCipherInfo(nameOrNid[, options])`.
pub(crate) fn get_cipher_info(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (name, options) = (arg(c, args, 0), arg(c, args, 1));
    let cipher = if let Some(text) = c.string_text(name)? {
        cipher_by_name(&text)
    } else if let Some(nid) = c.rooted_value(name).and_then(|value| value.as_number()).filter(|number| number.fract() == 0.0) {
        openssl::nid::Nid::from_raw(nid as i32).short_name().ok().and_then(cipher_by_name)
    } else {
        return Err(errors::invalid_type(c, name, "nameOrNid", "of type string or number"));
    };
    let Some(cipher) = cipher else {
        return Ok(c.undefined());
    };
    if !super::is_undefined(c, options) {
        super::object_argument(c, options, "options")?;
    }
    let key_length = uint_option(c, options, "keyLength")?;
    let iv_length = uint_option(c, options, "ivLength")?;
    let mode = cipher_mode(&cipher);
    if let Some(length) = key_length {
        let mut context = CipherCtx::new().map_err(|stack| errors::openssl(c, stack))?;
        let settable = context.encrypt_init(Some(&cipher), None, None).is_ok() && context.set_key_length(length).is_ok();
        if !settable {
            return Ok(c.undefined());
        }
    }
    if let Some(length) = iv_length {
        let valid = if is_aead(&cipher) {
            let mut context = CipherCtx::new().map_err(|stack| errors::openssl(c, stack))?;
            context.encrypt_init(Some(&cipher), None, None).is_ok() && context.set_iv_length(length).is_ok()
        } else {
            length == cipher.iv_length()
        };
        if !valid {
            return Ok(c.undefined());
        }
    }
    let result = c.object_rooted()?;
    let mode_text = c.string_rooted(mode_name(mode));
    put(c, result, "mode", mode_text)?;
    let short_name = cipher.nid().short_name().unwrap_or_default().to_ascii_lowercase();
    let name_text = c.string_rooted(&short_name);
    put(c, result, "name", name_text)?;
    let nid = c.number(f64::from(cipher.nid().as_raw()));
    put(c, result, "nid", nid)?;
    if mode != mode::STREAM {
        let block = c.number(cipher.block_size() as f64);
        put(c, result, "blockSize", block)?;
    }
    let iv = iv_length.unwrap_or(cipher.iv_length());
    if iv != 0 {
        let iv = c.number(iv as f64);
        put(c, result, "ivLength", iv)?;
    }
    let key = c.number(key_length.unwrap_or(cipher.key_length()) as f64);
    put(c, result, "keyLength", key)?;
    Ok(result)
}
