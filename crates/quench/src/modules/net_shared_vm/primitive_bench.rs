//! Direct idle-transport measurements; setup is outside timing. Linear TOD-123 owns
//! commands, input/precondition contracts and interpretation.

extern crate test;

use crate::host::NodeHost;
use quench_runtime::{Engine, Runtime};
use std::hint::black_box;
use test::Bencher;

const BATCH_POLLS: usize = 64;

#[bench]
fn primitive_empty_transport_poll_batch(b: &mut Bencher) {
    let mut runtime = Runtime::new(NodeHost::new(Vec::new()));
    let shared = runtime.host_mut().shared_state();
    let program = Engine::specialize("", "empty-transport-primitive").unwrap();
    b.iter(|| {
        for _ in 0..BATCH_POLLS {
            black_box(super::transport_poll::poll(
                black_box(&mut runtime),
                black_box(&program),
                black_box(&shared),
            ).unwrap());
        }
    });
}

#[bench]
fn primitive_control_bool_batch(b: &mut Bencher) {
    b.iter(|| {
        for _ in 0..BATCH_POLLS {
            black_box(false);
        }
    });
}
