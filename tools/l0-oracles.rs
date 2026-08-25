use std::{env, hint::black_box};

const DEFAULT_ITERATIONS: usize = 250_000;
const LONG_ITERATIONS: usize = 25_000_000;

#[derive(Clone, Copy)]
struct Collection {
    elements: [u64; 4],
    length: usize,
}

#[derive(Clone, Copy)]
struct Pair {
    car: u64,
    cdr: usize,
}

#[inline(never)]
fn increment(value: u64) -> u64 {
    value + 1
}

fn run(id: &str) -> u64 {
    match id {
        "u64-move" => {
            let mut x = black_box(1_u64);
            let mut y = 0;
            for _ in 0..LONG_ITERATIONS {
                y = black_box(x);
                x = black_box(y);
            }
            x + y
        }
        "load-proven" => {
            let value = black_box(1_u64);
            let mut total = 0_u64;
            for _ in 0..LONG_ITERATIONS {
                total = black_box(total).wrapping_add(black_box(value));
            }
            total
        }
        "getn-array-index" => {
            let collection = black_box(Collection {
                elements: [3, 5, 7, 11],
                length: 4,
            });
            let mut total = 0_u64;
            for index in 0..DEFAULT_ITERATIONS {
                total = black_box(total)
                    .wrapping_add(black_box(collection.elements[index & 3]));
            }
            total
        }
        "getn-array-length" => {
            let collection = black_box(Collection {
                elements: [0; 4],
                length: 0,
            });
            let mut total = 0_usize;
            for _ in 0..LONG_ITERATIONS {
                total = black_box(total).wrapping_add(black_box(collection.length));
            }
            total as u64
        }
        "call-fp" => {
            let mut result = 0;
            for _ in 0..DEFAULT_ITERATIONS {
                result = increment(black_box(result));
            }
            result
        }
        "pair-car-cdr" => {
            let pairs = black_box([Pair { car: 1, cdr: 1 }, Pair { car: 2, cdr: 0 }]);
            let mut pair = 0;
            let mut sum = 0_u64;
            for _ in 0..DEFAULT_ITERATIONS {
                sum = black_box(sum).wrapping_add(black_box(pairs[pair].car));
                pair = black_box(pairs[pair].cdr);
            }
            sum
        }
        "lastIndex-word" => {
            let haystack = black_box(b"x");
            let mut last_index = 0;
            let mut matches = 0;
            for _ in 0..DEFAULT_ITERATIONS {
                last_index = 0;
                if black_box(haystack).get(last_index) == Some(&b'x') {
                    last_index += 1;
                    matches = black_box(matches) + 1;
                }
            }
            matches + last_index as u64
        }
        _ => panic!("unknown L0 oracle: {id}"),
    }
}

fn main() {
    let id = env::args().nth(1).expect("missing L0 oracle id");
    println!("{}", black_box(run(&id)));
}
