// The spec testsuite's host module, as the package `spectest`.
const print = () => {};
export { print, print as print_i32, print as print_i64, print as print_f32, print as print_f64,
  print as print_i32_f32, print as print_f64_f64 };
export const global_i32 = new WebAssembly.Global({ value: "i32" }, 666);
export const global_i64 = new WebAssembly.Global({ value: "i64" }, 666n);
export const global_f32 = new WebAssembly.Global({ value: "f32" }, 666.6);
export const global_f64 = new WebAssembly.Global({ value: "f64" }, 666.6);
export const table = new WebAssembly.Table({ initial: 10, maximum: 20, element: "anyfunc" });
export const memory = new WebAssembly.Memory({ initial: 1, maximum: 2 });
