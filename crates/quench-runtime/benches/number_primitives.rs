#![feature(test)]

extern crate test;

#[path = "support/number_inputs.rs"]
mod number_inputs;

use number_inputs::Inputs;
use quench_runtime::Value;
use std::hint::black_box;
use test::Bencher;

const CHAIN_FACTOR: f64 = 0.5;

fn encode(b: &mut Bencher, inputs: Inputs) {
    let values = inputs.numbers();
    b.iter(|| {
        for &value in &values {
            black_box(Value::number(black_box(value)));
        }
    });
}

fn chain<const PACK: bool>(b: &mut Bencher, inputs: Inputs) {
    let inputs = inputs.numbers();
    b.iter(|| {
        let mut value = black_box(0.0);
        for &input in &inputs {
            let roundtrip = if PACK {
                Value::number(black_box(value)).as_number().unwrap()
            } else {
                black_box(value)
            };
            value = roundtrip * CHAIN_FACTOR + black_box(input);
        }
        black_box(value);
    });
}

macro_rules! kernels {
    ($($name:ident: $measure:ident $(::<$pack:literal>)? ($inputs:ident)),+ $(,)?) => {
        $(#[bench]
        fn $name(b: &mut Bencher) { $measure $(::<$pack>)? (b, Inputs::$inputs); })+
    };
}

kernels! {
    number_encode_int32: encode(Integers),
    number_encode_unit_fraction: encode(UnitFractions),
    number_encode_large_fraction: encode(Fractions),
    number_encode_mixed: encode(Mixed),
    number_encode_edges: encode(Boundaries),
    number_roundtrip_unit_chain: chain::<true>(UnitFractions),
    number_roundtrip_large_chain: chain::<true>(Fractions),
    number_control_unit_chain: chain::<false>(UnitFractions),
    number_control_large_chain: chain::<false>(Fractions),
}
