//! Compiler-owned linked program catalogs, independent of the embedding host.

/// One compiler-owned UTF-8 pool; all ranges retain their exact original text.
#[macro_export]
macro_rules! linked_source_pool {
    ($lookup:ident, $path:literal) => {
        const fn $lookup(start: usize, len: usize) -> &'static str {
            const TEXT: &str = include_str!($path);
            TEXT.split_at(start).1.split_at(len).0
        }
    };
}

/// Bind the compiler's metadata and static entry tables for an explicit host. The
/// generated invocation contains only data; this macro owns the loading rules.
#[macro_export]
macro_rules! linked_native_sources {
    (host $host:ty; entry $entry:ident: $entry_symbol:ident => ($entry_link:literal, $entry_metadata:literal);
     resolver $resolve:ident {
         $($symbol:ident => ($link:literal, $metadata:literal, $name:literal, [$($part:expr),*] $(, source $source:expr)?),)*
     } $(errors {
         $(($error_name:literal, [$($error_part:expr),*]) => $message:expr,)*
     })?) => {
        pub const NATIVE_SOURCE_COUNT: usize = (&[$(stringify!($symbol)),*] as &[&str]).len()
            + (&[$($(stringify!($error_name)),*)?] as &[&str]).len();
        /// # Safety
        /// Metadata, host ABI and all entry tables must be emitted together by
        /// this compiler, linked unmodified, and kept alive for the process.
        pub unsafe fn $entry() -> Result<$crate::NativeJsProgram<$host>, String> {
            unsafe extern "C" { #[link_name = $entry_link] static $entry_symbol: u8; }
            let bytes: $crate::KeptBytes = std::rc::Rc::new(&include_bytes!($entry_metadata)[..]);
            let program = $crate::ResidualProgram::from_binary_bytes(bytes)?;
            // SAFETY: the compiler emits the metadata and this linked table together.
            unsafe { $crate::NativeJsProgram::from_linked_image(program, std::ptr::addr_of!($entry_symbol)) }
        }
        /// # Safety
        /// Every catalog record must meet the linked-image contract above.
        pub unsafe fn $resolve(namespace: &$crate::NativeProgramNamespace, name: &str, parts: &[&str]) -> Result<Option<$crate::NativeJsProgram<$host>>, $crate::NativeSourceError> {
            $(if name == $name && parts == (&[$($part),*] as &[&str]) {
                unsafe extern "C" { #[link_name = $link] static $symbol: u8; }
                let bytes: $crate::KeptBytes = std::rc::Rc::new(&include_bytes!($metadata)[..]);
                let program = $crate::linked_native_sources!(@decode namespace, bytes $(, $source)?)?;
                // SAFETY: as above, including the compiler-wide atom namespace.
                return unsafe { $crate::NativeJsProgram::from_linked_image(program, std::ptr::addr_of!($symbol)) }.map(Some).map_err(Into::into);
            })*
            $($(if name == $error_name && parts == (&[$($error_part),*] as &[&str]) {
                return Err($crate::NativeSourceError::Syntax($message.to_owned()));
            })*)?
            Ok(None)
        }
    };
    (@decode $namespace:ident, $bytes:ident) => { $namespace.decode($bytes) };
    (@decode $namespace:ident, $bytes:ident, $source:expr) => { $namespace.decode_with_static_source($bytes, $source) };
}
