//! Tests of programs that run through the engine (and the Intl layer), each held to the
//! interpreter's run or to the core's rooting rules (moved from the core's library tests, whose
//! binary does not link the layers).
mod ahead_behaviour;
mod compiled_wasm1;
mod compiled_wasm2;
mod compiled_wasm3;
mod compiled_wasm4;
mod compiled_wasm5;
mod compiled_wasm6;
mod compiled_wasm_host;
mod eager_images;
mod intl_rooting;
mod js_jit;
mod lane_bodies1;
mod lane_bodies2;
mod lane_bodies3;
mod lane_bodies4;
mod lane_bodies5;
mod lane_bodies6;
mod native_api;
mod native_images;
mod object_registers;
#[cfg(feature = "jit")]
mod select_lowering;
mod shared_osr1;
mod shared_osr2;
mod shared_osr3;
mod shared_osr4;
mod shared_osr5;
mod wasm_indirect;
