//! Link configuration derived from the compilation target (see `../link_layout.rs`; the
//! symbol orders are trained by `src/bin/symbol-order.rs`).

#[path = "../link_layout.rs"]
mod link_layout;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=../link_layout.rs");
    link_layout::link_binaries(true);
}
