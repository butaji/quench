//! Node-API fixture using the pinned headers' C ABI, implemented in Rust.
use std::ffi::{c_char, c_void};
type Env = *mut c_void;
type Value = *mut c_void;
type CallbackInfo = *mut c_void;
type Callback = unsafe extern "C" fn(Env, CallbackInfo) -> Value;
const NAPI_OK: i32 = 0;
const NAPI_AUTO_LENGTH: usize = usize::MAX;
unsafe extern "C" {
    fn napi_create_int32(env: Env, value: i32, result: *mut Value) -> i32;
    fn napi_get_value_int32(env: Env, value: Value, result: *mut i32) -> i32;
    fn napi_set_named_property(env: Env, target: Value, name: *const c_char, value: Value) -> i32;
    fn napi_get_cb_info(env: Env, info: CallbackInfo, argc: *mut usize, argv: *mut Value, this_arg: *mut Value, data: *mut *mut c_void) -> i32;
    fn napi_create_function(env: Env, name: *const c_char, length: usize, callback: Callback, data: *mut c_void, result: *mut Value) -> i32;
}
unsafe extern "C" fn increment(env: Env, info: CallbackInfo) -> Value {
    let mut count = 1;
    let mut argument = std::ptr::null_mut();
    let mut number = 0;
    let mut result = std::ptr::null_mut();
    if unsafe { napi_get_cb_info(env, info, &mut count, &mut argument, std::ptr::null_mut(), std::ptr::null_mut()) } != NAPI_OK
        || unsafe { napi_get_value_int32(env, argument, &mut number) } != NAPI_OK
        || unsafe { napi_create_int32(env, number.wrapping_add(1), &mut result) } != NAPI_OK {
        return std::ptr::null_mut();
    }
    result
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_register_module_v1(env: Env, exports: Value) -> Value {
    let mut answer = std::ptr::null_mut();
    let mut function = std::ptr::null_mut();
    if unsafe { napi_create_int32(env, 42, &mut answer) } != NAPI_OK
        || unsafe { napi_set_named_property(env, exports, c"answer".as_ptr(), answer) } != NAPI_OK
        || unsafe { napi_create_function(env, c"increment".as_ptr(), NAPI_AUTO_LENGTH, increment, std::ptr::null_mut(), &mut function) } != NAPI_OK
        || unsafe { napi_set_named_property(env, exports, c"increment".as_ptr(), function) } != NAPI_OK {
        return std::ptr::null_mut();
    }
    exports
}
#[unsafe(no_mangle)]
pub extern "C" fn node_api_module_get_api_version_v1() -> i32 {
    env!("QUENCH_FIXTURE_NODE_API_VERSION").parse().expect("pinned Node-API module version")
}
