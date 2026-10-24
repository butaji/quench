//! Real incremental EVP contexts. No recorded input/chunks and no guest roots.
//! Arc<Digest> keeps the fetched implementation alive beyond registry refresh.
use super::crypto_digest::{Digest, DigestError};
use foreign_types::{ForeignType, ForeignTypeRef};
use openssl::{error::ErrorStack, md_ctx::MdCtx};
use openssl_sys::{EVP_MD_CTX, HMAC_CTX};
use std::{
    ffi::{c_int, c_void},
    ptr::{self, NonNull},
    sync::Arc,
};
unsafe extern "C" {
    fn EVP_MD_CTX_copy_ex(dst: *mut EVP_MD_CTX, src: *const EVP_MD_CTX) -> c_int;
}
fn output(size: usize) -> Result<Vec<u8>, DigestError> {
    let mut out = Vec::new();
    out.try_reserve_exact(size)
        .map_err(|_| DigestError::Allocation)?;
    out.resize(size, 0);
    Ok(out)
}
pub struct HashState {
    ctx: MdCtx,
    digest: Arc<Digest>,
    length: usize,
    cached: Option<Vec<u8>>,
}
impl HashState {
    pub fn new(
        digest: Arc<Digest>,
        length: Option<usize>,
        function_name: Option<&[u8]>,
        customization: Option<&[u8]>,
    ) -> Result<Self, DigestError> {
        // Node26 CShakeOptions::Initialize explicitly returns false before
        // OpenSSL4, even for an explicitly supplied empty option. OpenSSL3.5.9
        // has no CSHAKE implementation/parameter ABI; never ignore options.
        if function_name.is_some() || customization.is_some() {
            return Err(DigestError::Unsupported);
        }
        let mut ctx = MdCtx::new()?;
        ctx.digest_init(digest.native_md())?;
        let default = digest.size();
        let length = length.unwrap_or(default);
        if !digest.is_xof() && length != default {
            return Err(DigestError::InvalidLength {
                expected: default,
                actual: length,
            });
        }
        Ok(Self {
            ctx,
            digest,
            length,
            cached: None,
        })
    }
    pub fn copy(&self, length: Option<usize>) -> Result<Self, DigestError> {
        let copy = Self::new(self.digest.clone(), length, None, None)?;
        if unsafe { EVP_MD_CTX_copy_ex(copy.ctx.as_ptr(), self.ctx.as_ptr()) } != 1 {
            return Err(DigestError::OpenSsl(ErrorStack::get()));
        }
        Ok(copy)
    }
    pub fn update(&mut self, input: &[u8]) -> Result<bool, DigestError> {
        // Native Node leaves the context alive after digest. Its public SDK
        // owns finalized guards; native update still delegates to OpenSSL.
        Ok(unsafe {
            openssl_sys::EVP_DigestUpdate(self.ctx.as_ptr(), input.as_ptr().cast(), input.len())
        } == 1)
    }
    pub fn digest(&mut self) -> Result<Vec<u8>, DigestError> {
        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }
        let mut out = output(self.length)?;
        // Node skips finalization for zero output length, avoiding old SHA3
        // provider zero-squeeze bugs and retaining the live context.
        if self.length != 0 {
            if self.digest.is_xof() {
                self.ctx.digest_final_xof(&mut out)?;
            } else {
                let length = self.ctx.digest_final(&mut out)?;
                out.truncate(length);
            }
        }
        self.cached = Some(out.clone());
        Ok(out)
    }
}
pub struct HmacState {
    ctx: Option<NonNull<HMAC_CTX>>,
    _digest: Arc<Digest>,
}
impl Drop for HmacState {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            unsafe { openssl_sys::HMAC_CTX_free(ctx.as_ptr()) }
        }
    }
}
// OpenSSL contexts may move between threads with exclusive ownership. There is
// deliberately no Sync implementation; every mutating operation requires &mut.
unsafe impl Send for HmacState {}
impl HmacState {
    pub fn new(digest: Arc<Digest>, key: &[u8]) -> Result<Self, DigestError> {
        let length = c_int::try_from(key.len()).map_err(|_| DigestError::Allocation)?;
        let ctx = NonNull::new(unsafe { openssl_sys::HMAC_CTX_new() })
            .ok_or_else(|| DigestError::OpenSsl(ErrorStack::get()))?;
        let state = Self {
            ctx: Some(ctx),
            _digest: digest,
        };
        // A non-null zero-length key, exactly like Node's key="" override.
        let empty = [0u8];
        let pointer = if key.is_empty() {
            empty.as_ptr()
        } else {
            key.as_ptr()
        };
        if unsafe {
            openssl_sys::HMAC_Init_ex(
                ctx.as_ptr(),
                pointer.cast::<c_void>(),
                length,
                state._digest.native_md().as_ptr(),
                ptr::null_mut(),
            )
        } != 1
        {
            return Err(DigestError::OpenSsl(ErrorStack::get()));
        }
        Ok(state)
    }
    pub fn update(&mut self, input: &[u8]) -> Result<bool, DigestError> {
        let Some(ctx) = self.ctx else {
            return Ok(false);
        };
        Ok(unsafe { openssl_sys::HMAC_Update(ctx.as_ptr(), input.as_ptr(), input.len()) } == 1)
    }
    pub fn digest(&mut self) -> Result<Vec<u8>, DigestError> {
        let Some(ctx) = self.ctx else {
            return Ok(Vec::new());
        };
        let mut out = output(self._digest.size())?;
        let mut length = u32::try_from(out.len()).map_err(|_| DigestError::Allocation)?;
        let ok =
            unsafe { openssl_sys::HMAC_Final(ctx.as_ptr(), out.as_mut_ptr(), &mut length) } == 1;
        // Native Node resets the actual HMAC context on both success/failure.
        self.ctx = None;
        unsafe { openssl_sys::HMAC_CTX_free(ctx.as_ptr()) };
        if !ok {
            return Err(DigestError::OpenSsl(ErrorStack::get()));
        }
        out.truncate(length as usize);
        Ok(out)
    }
}
