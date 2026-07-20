//! Function types - ValueFunction, NativeFunction, and NativeConstructor.

mod native_constructor;
mod native_function;
mod value_function;

#[cfg(test)]
#[cfg(test)]
mod tests;

pub use native_constructor::{ConstructorAccessor, NativeConstructor};
pub use native_function::{NativeFn, NativeFunction};
pub(crate) use value_function::expected_argument_count;
pub use value_function::ValueFunction;
