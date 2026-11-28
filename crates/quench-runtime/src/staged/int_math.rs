//! Integer operations every family shares: the JS rows and the Wasm rows name the same one, and
//! the b-jit stencil build mounts this file in both stencil families (it uses nothing but core).

/// `Math.imul` and Wasm's 32-bit lane products: the wrapping product of two int32 values.
#[inline(always)]
pub fn int_multiply(left: i32, right: i32) -> i32 {
    left.wrapping_mul(right)
}
