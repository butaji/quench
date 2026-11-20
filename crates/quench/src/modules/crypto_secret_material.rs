//! Cleansed native-owned temporary secret material shared by crypto operations.
pub(crate) struct Material(Vec<u8>);
impl Material {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}
impl std::ops::Deref for Material {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}
impl std::ops::DerefMut for Material {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}
pub(crate) fn cleanse(bytes: &mut [u8]) {
    unsafe extern "C" {
        fn OPENSSL_cleanse(pointer: *mut std::ffi::c_void, length: usize);
    }
    unsafe { OPENSSL_cleanse(bytes.as_mut_ptr().cast(), bytes.len()) }
}
impl Drop for Material {
    fn drop(&mut self) {
        cleanse(&mut self.0);
    }
}
