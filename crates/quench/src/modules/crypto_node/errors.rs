//! The errors the crypto module raises, with Node's codes, classes and messages.

use super::{Context, Outcome};
use crate::modules::node_errors_shared_vm as node_errors;
use crate::modules::util_shared_vm;
use quench_runtime::{RootId, RootedError};

/// A crypto error, by its Node code. `Plain` errors carry no code (the native layer's).
pub(crate) enum CryptoError<'a> {
    /// `ERR_CRYPTO_INVALID_DIGEST`, naming the digest when Node does.
    InvalidDigest(Option<&'a str>),
    /// `ERR_OSSL_EVP_INVALID_DIGEST`.
    InvalidDigestUsed,
    /// `ERR_CRYPTO_INVALID_STATE`, naming the operation when Node does.
    InvalidState(Option<&'a str>),
    /// `ERR_CRYPTO_INVALID_STATE` with the native layer's message.
    InvalidStateMessage(&'a str),
    UnknownCipher,
    InvalidKeyLength,
    /// `ERR_CRYPTO_INVALID_KEYLEN` with the native layer's message.
    InvalidKeyLengthMessage(&'a str),
    /// `ERR_CRYPTO_INVALID_KEYTYPE` with the native layer's message.
    InvalidKeyType(&'a str),
    InvalidIv,
    /// `ERR_CRYPTO_INVALID_AUTH_TAG` with its formatted message.
    InvalidAuthTag(String),
    InvalidMessageLength,
    TimingSafeEqualLength,
    /// `ERR_CRYPTO_INVALID_SCRYPT_PARAMS`, with the native reason when there is one.
    InvalidScryptParams(Option<String>),
    ScryptInvalidParameter,
    InvalidCurve,
    /// `ERR_CRYPTO_ECDH_INVALID_FORMAT`, with the format as given.
    EcdhInvalidFormat(String),
    EcdhInvalidPublicKey,
    /// `ERR_CRYPTO_OPERATION_FAILED` with the native layer's message.
    OperationFailed(&'a str),
    /// `ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE`: the type received, and the one expected.
    InvalidKeyObjectType(&'a str, &'a str),
    /// `ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS`: the encoding, and why.
    IncompatibleKeyOptions(&'a str, &'a str),
    /// `ERR_CRYPTO_INCOMPATIBLE_KEY`: what is incompatible, and how.
    IncompatibleKey(&'a str, &'a str),
    InvalidJwk,
    JwkUnsupportedKeyType,
    UnknownDhGroup,
    SignKeyRequired,
    /// `ERR_CRYPTO_ENGINE_UNKNOWN`, naming the engine.
    EngineUnknown(&'a str),
    UnsupportedOperation,
    MissingPassphrase,
    /// `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM`.
    FeatureUnavailable(&'a str),
    /// `ERR_MISSING_OPTION`, naming what is required.
    MissingOption(&'a str),
    /// `ERR_MISSING_ARGS` with its formatted message.
    MissingArgs(&'a str),
    /// `ERR_INVALID_THIS`, naming the expected receiver class.
    InvalidThis(&'a str),
    IllegalConstructor,
    /// `ERR_UNKNOWN_ENCODING`, with the encoding as given.
    UnknownEncoding(String),
    /// `ERR_INVALID_ARG_TYPE` with a message the native layer formats (no received value).
    InvalidArgTypeMessage(String),
    /// `ERR_INCOMPATIBLE_OPTION_PAIR` with its formatted message.
    IncompatibleOptionPair(String),
    /// `ERR_MISSING_OPTION`, `ERR_MISSING_ARGS` and `ERR_INVALID_ARG_VALUE` with WebIDL's messages.
    MissingOptionMessage(String),
    MissingArgsMessage(String),
    InvalidArgValueMessage(String),
    /// `ERR_OUT_OF_RANGE` with its formatted message.
    OutOfRange(String),
    /// A codeless `Error`, as the native layer throws it.
    Plain(&'a str),
    /// A codeless `TypeError`.
    PlainType(&'a str),
}

#[derive(Clone, Copy)]
enum ErrorClass {
    Error,
    TypeError,
    RangeError,
}

impl CryptoError<'_> {
    fn code(&self) -> Option<&'static str> {
        Some(match self {
            Self::InvalidDigest(_) => "ERR_CRYPTO_INVALID_DIGEST",
            Self::InvalidDigestUsed => "ERR_OSSL_EVP_INVALID_DIGEST",
            Self::InvalidState(_) | Self::InvalidStateMessage(_) => "ERR_CRYPTO_INVALID_STATE",
            Self::UnknownCipher => "ERR_CRYPTO_UNKNOWN_CIPHER",
            Self::InvalidKeyLength | Self::InvalidKeyLengthMessage(_) => "ERR_CRYPTO_INVALID_KEYLEN",
            Self::InvalidKeyType(_) => "ERR_CRYPTO_INVALID_KEYTYPE",
            Self::InvalidIv => "ERR_CRYPTO_INVALID_IV",
            Self::InvalidAuthTag(_) => "ERR_CRYPTO_INVALID_AUTH_TAG",
            Self::InvalidMessageLength => "ERR_CRYPTO_INVALID_MESSAGELEN",
            Self::TimingSafeEqualLength => "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH",
            Self::InvalidScryptParams(_) => "ERR_CRYPTO_INVALID_SCRYPT_PARAMS",
            Self::ScryptInvalidParameter => "ERR_CRYPTO_SCRYPT_INVALID_PARAMETER",
            Self::InvalidCurve => "ERR_CRYPTO_INVALID_CURVE",
            Self::EcdhInvalidFormat(_) => "ERR_CRYPTO_ECDH_INVALID_FORMAT",
            Self::EcdhInvalidPublicKey => "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY",
            Self::OperationFailed(_) => "ERR_CRYPTO_OPERATION_FAILED",
            Self::InvalidKeyObjectType(..) => "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            Self::IncompatibleKeyOptions(..) => "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS",
            Self::IncompatibleKey(..) => "ERR_CRYPTO_INCOMPATIBLE_KEY",
            Self::InvalidJwk => "ERR_CRYPTO_INVALID_JWK",
            Self::JwkUnsupportedKeyType => "ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE",
            Self::UnknownDhGroup => "ERR_CRYPTO_UNKNOWN_DH_GROUP",
            Self::SignKeyRequired => "ERR_CRYPTO_SIGN_KEY_REQUIRED",
            Self::EngineUnknown(_) => "ERR_CRYPTO_ENGINE_UNKNOWN",
            Self::UnsupportedOperation => "ERR_CRYPTO_UNSUPPORTED_OPERATION",
            Self::MissingPassphrase => "ERR_MISSING_PASSPHRASE",
            Self::FeatureUnavailable(_) => "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
            Self::MissingOption(_) => "ERR_MISSING_OPTION",
            Self::MissingArgs(_) => "ERR_MISSING_ARGS",
            Self::InvalidThis(_) => "ERR_INVALID_THIS",
            Self::IllegalConstructor => "ERR_ILLEGAL_CONSTRUCTOR",
            Self::UnknownEncoding(_) => "ERR_UNKNOWN_ENCODING",
            Self::InvalidArgTypeMessage(_) => "ERR_INVALID_ARG_TYPE",
            Self::IncompatibleOptionPair(_) => "ERR_INCOMPATIBLE_OPTION_PAIR",
            Self::OutOfRange(_) => "ERR_OUT_OF_RANGE",
            Self::MissingOptionMessage(_) => "ERR_MISSING_OPTION",
            Self::MissingArgsMessage(_) => "ERR_MISSING_ARGS",
            Self::InvalidArgValueMessage(_) => "ERR_INVALID_ARG_VALUE",
            Self::Plain(_) | Self::PlainType(_) => return None,
        })
    }

    fn class(&self) -> ErrorClass {
        match self {
            Self::InvalidDigest(_)
            | Self::InvalidIv
            | Self::InvalidCurve
            | Self::EcdhInvalidFormat(_)
            | Self::InvalidKeyObjectType(..)
            | Self::InvalidJwk
            | Self::MissingPassphrase
            | Self::MissingOption(_)
            | Self::MissingArgs(_)
            | Self::InvalidThis(_)
            | Self::IllegalConstructor
            | Self::UnknownEncoding(_)
            | Self::InvalidArgTypeMessage(_)
            | Self::IncompatibleOptionPair(_)
            | Self::MissingOptionMessage(_)
            | Self::MissingArgsMessage(_)
            | Self::InvalidArgValueMessage(_)
            | Self::PlainType(_) => ErrorClass::TypeError,
            Self::InvalidKeyLength
            | Self::InvalidKeyLengthMessage(_)
            | Self::InvalidKeyType(_)
            | Self::InvalidAuthTag(_)
            | Self::InvalidMessageLength
            | Self::TimingSafeEqualLength
            | Self::InvalidScryptParams(_)
            | Self::OutOfRange(_) => ErrorClass::RangeError,
            _ => ErrorClass::Error,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::InvalidDigest(Some(name)) => format!("Invalid digest: {name}"),
            Self::InvalidDigest(None) => "Invalid digest".to_owned(),
            Self::InvalidDigestUsed => "Invalid digest used".to_owned(),
            Self::InvalidState(Some(operation)) => format!("Invalid state for operation {operation}"),
            Self::InvalidState(None) => "Invalid state".to_owned(),
            Self::UnknownCipher => "Unknown cipher".to_owned(),
            Self::InvalidKeyLength => "Invalid key length".to_owned(),
            Self::InvalidIv => "Invalid initialization vector".to_owned(),
            Self::InvalidMessageLength => "Invalid message length".to_owned(),
            Self::TimingSafeEqualLength => "Input buffers must have the same byte length".to_owned(),
            Self::InvalidScryptParams(Some(reason)) => format!("Invalid scrypt params: {reason}"),
            Self::InvalidScryptParams(None) => "Invalid scrypt params".to_owned(),
            Self::ScryptInvalidParameter => "Invalid scrypt parameter".to_owned(),
            Self::InvalidCurve => "Invalid EC curve name".to_owned(),
            Self::EcdhInvalidFormat(format) => format!("Invalid ECDH format: {format}"),
            Self::EcdhInvalidPublicKey => "Public key is not valid for specified curve".to_owned(),
            Self::InvalidKeyObjectType(actual, expected) => format!("Invalid key object type {actual}, expected {expected}."),
            Self::IncompatibleKeyOptions(encoding, reason) => format!("The selected key encoding {encoding} {reason}."),
            Self::IncompatibleKey(what, how) => format!("Incompatible {what}: {how}"),
            Self::InvalidJwk => "Invalid JWK data".to_owned(),
            Self::JwkUnsupportedKeyType => "Unsupported JWK Key Type.".to_owned(),
            Self::UnknownDhGroup => "Unknown DH group".to_owned(),
            Self::SignKeyRequired => "No key provided to sign".to_owned(),
            Self::EngineUnknown(id) => format!("Engine \"{id}\" was not found"),
            Self::UnsupportedOperation => "Unsupported crypto operation".to_owned(),
            Self::MissingPassphrase => "Passphrase required for encrypted key".to_owned(),
            Self::FeatureUnavailable(feature) => format!("{feature} is not available on this platform"),
            Self::MissingOption(name) => format!("{name} is required"),
            Self::InvalidThis(class) => format!("Value of \"this\" must be of type {class}"),
            Self::IllegalConstructor => "Illegal constructor".to_owned(),
            Self::UnknownEncoding(encoding) => format!("Unknown encoding: {encoding}"),
            Self::InvalidAuthTag(message)
            | Self::InvalidArgTypeMessage(message)
            | Self::IncompatibleOptionPair(message)
            | Self::MissingOptionMessage(message)
            | Self::MissingArgsMessage(message)
            | Self::InvalidArgValueMessage(message)
            | Self::OutOfRange(message) => message.clone(),
            Self::InvalidStateMessage(message)
            | Self::InvalidKeyLengthMessage(message)
            | Self::InvalidKeyType(message)
            | Self::OperationFailed(message)
            | Self::MissingArgs(message)
            | Self::Plain(message)
            | Self::PlainType(message) => (*message).to_owned(),
        }
    }
}

/// The error, as Node's `new ERR_*()` makes it (not thrown).
pub(crate) fn create(c: &mut Context<'_>, error: CryptoError<'_>) -> Outcome {
    let message = error.message();
    let class = error.class();
    let value = match class {
        ErrorClass::Error => c.error_rooted(&message)?,
        ErrorClass::TypeError => c.type_error_rooted(&message)?,
        ErrorClass::RangeError => c.range_error_rooted(&message)?,
    };
    if let Some(code) = error.code() {
        match class {
            ErrorClass::Error => node_errors::decorate_error(c, value, code)?,
            ErrorClass::TypeError => node_errors::decorate_type_error(c, value, code)?,
            ErrorClass::RangeError => node_errors::decorate_range_error(c, value, code)?,
        }
    }
    Ok(value)
}

pub(crate) fn throw(c: &mut Context<'_>, error: CryptoError<'_>) -> RootedError {
    match create(c, error) {
        Ok(value) => c.throw(value),
        Err(error) => error,
    }
}

/// `ERR_INVALID_ARG_TYPE` with Node's formatted list of accepted types.
pub(crate) fn invalid_type(c: &mut Context<'_>, value: RootId, name: &str, expected: &str) -> RootedError {
    match util_shared_vm::argument_type_error_with_expected(c, value, name, expected) {
        Ok(error) | Err(error) => error,
    }
}

/// `ERR_INVALID_ARG_VALUE`: `The argument 'name' reason. Received <inspected>`.
pub(crate) fn invalid_value(c: &mut Context<'_>, value: RootId, name: &str, reason: &str) -> RootedError {
    match util_shared_vm::argument_value_error(c, value, name, reason) {
        Ok(error) | Err(error) => error,
    }
}

/// `ERR_OUT_OF_RANGE`: `The value of "name" is out of range. It must be range. Received value`.
pub(crate) fn out_of_range(c: &mut Context<'_>, name: &str, range: &str, value: RootId) -> RootedError {
    let received = match util_shared_vm::range_error_received(c, value) {
        Ok(received) => received,
        Err(error) => return error,
    };
    let message = format!("The value of \"{name}\" is out of range. It must be {range}. Received {received}");
    throw(c, CryptoError::OutOfRange(message))
}

/// The OpenSSL error queue as Node decorates it (`library`, `reason`, `code`).
pub(crate) fn openssl(c: &mut Context<'_>, stack: openssl::error::ErrorStack) -> RootedError {
    crate::modules::crypto_error_shared_vm::decorated(c, stack)
}

/// The value a thrown error carries, for a callback or a rejection.
pub(crate) fn caught(error: RootedError) -> Result<RootId, RootedError> {
    match error {
        RootedError { exception: Some(exception), .. } => Ok(exception),
        error => Err(error),
    }
}

/// A `DOMException` with a WebCrypto name (`NotSupportedError`, `OperationError`, ...).
pub(crate) fn dom(c: &mut Context<'_>, message: &str, name: &str) -> RootedError {
    let message = c.string_rooted(message);
    let name = c.string_rooted(name);
    match crate::modules::dom_exception_shared_vm::create(c, message, name) {
        Ok(value) => c.throw(value),
        Err(error) => error,
    }
}
