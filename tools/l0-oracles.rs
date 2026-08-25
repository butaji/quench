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
            for index in 0..LONG_ITERATIONS {
                total = black_box(total).wrapping_add(black_box(collection.elements[index & 3]));
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
            for _ in 0..LONG_ITERATIONS {
                result = increment(black_box(result));
            }
            result
        }
        "pair-car-cdr" => {
            let pairs = black_box([Pair { car: 1, cdr: 1 }, Pair { car: 2, cdr: 0 }]);
            let mut pair = 0;
            let mut sum = 0_u64;
            for _ in 0..LONG_ITERATIONS {
                sum = black_box(sum).wrapping_add(black_box(pairs[pair].car));
                pair = black_box(pairs[pair].cdr);
            }
            sum
        }
        "lastIndex-word" => {
            let haystack = black_box(b"x");
            let mut last_index = 0;
            let mut matches = 0;
            for _ in 0..LONG_ITERATIONS {
                last_index = 0;
                if black_box(haystack).get(last_index) == Some(&b'x') {
                    last_index += 1;
                    matches = black_box(matches) + 1;
                }
            }
            matches + last_index as u64
        }
        "packed-f64-jacobi" => {
            const WIDTH: usize = 128;
            const HEIGHT: usize = 128;
            const ROW_SIZE: usize = WIDTH + 2;
            let mut x = vec![1.01_f64; ROW_SIZE * (HEIGHT + 2)];
            let x0 = vec![2.03_f64; ROW_SIZE * (HEIGHT + 2)];
            for _ in 0..100 {
                for _ in 0..20 {
                    for j in 1..=HEIGHT {
                        let (mut last, mut current, mut next) =
                            ((j - 1) * ROW_SIZE, j * ROW_SIZE + 1, (j + 1) * ROW_SIZE);
                        let mut last_x = x[j * ROW_SIZE];
                        for _ in 0..WIDTH {
                            let value = (x0[current]
                                + 0.1 * (last_x + x[current + 1] + x[last + 1] + x[next + 1]))
                                * 0.5;
                            x[current] = value;
                            last_x = value;
                            current += 1;
                            last += 1;
                            next += 1;
                        }
                    }
                }
            }
            black_box(x[HEIGHT * ROW_SIZE + WIDTH]).to_bits()
        }
        "packed-f64-advect" => {
            const WIDTH: usize = 4096;
            const HEIGHT: usize = 64;
            const ROW_SIZE: usize = WIDTH + 2;
            let mut d = vec![0.0_f64; ROW_SIZE * (HEIGHT + 2)];
            let d0 = vec![1.25_f64; d.len()];
            let u = vec![0.001_f64; d.len()];
            let v = vec![0.001_f64; d.len()];
            for _ in 0..100 {
                for j in 1..=HEIGHT {
                    let mut pos = j * ROW_SIZE;
                    for i in 1..=WIDTH {
                        pos += 1;
                        let x = (i as f64 - 0.001 * WIDTH as f64 * u[pos])
                            .clamp(0.5, WIDTH as f64 + 0.5);
                        let y = (j as f64 - 0.001 * HEIGHT as f64 * v[pos])
                            .clamp(0.5, HEIGHT as f64 + 0.5);
                        let (i0, j0) = (x as usize, y as usize);
                        let (s1, t1) = (x - i0 as f64, y - j0 as f64);
                        let (row1, row2) = (j0 * ROW_SIZE, (j0 + 1) * ROW_SIZE);
                        d[pos] = (1.0 - s1) * ((1.0 - t1) * d0[i0 + row1] + t1 * d0[i0 + row2])
                            + s1 * ((1.0 - t1) * d0[i0 + 1 + row1] + t1 * d0[i0 + 1 + row2]);
                    }
                }
            }
            black_box(d[ROW_SIZE + 1]).to_bits()
        }
        "packed-f64-add-fields" => {
            const SIZE: usize = 262_144;
            let mut x = vec![1.25_f64; SIZE];
            let source = vec![0.5_f64; SIZE];
            for _ in 0..1_000 {
                for (value, source) in x.iter_mut().zip(&source) {
                    *value += 0.01 * source;
                }
            }
            black_box(x[0]).to_bits()
        }
        "packed-f64-fill3" => {
            const SIZE: usize = 262_144;
            let mut density = vec![1.25_f64; SIZE];
            let mut u = vec![2.5_f64; SIZE];
            let mut v = vec![3.75_f64; SIZE];
            for round in 0..1_000 {
                density[0] = round as f64 + 1.0;
                u[0] = density[0];
                v[0] = density[0];
                density.fill(0.0);
                u.fill(0.0);
                v.fill(0.0);
                black_box((&density, &u, &v));
            }
            black_box(density[0] + u[0] + v[0]).to_bits()
        }
        "packed-f64-boundary" => {
            const WIDTH: usize = 4096;
            const HEIGHT: usize = 64;
            const ROW_SIZE: usize = WIDTH + 2;
            let mut x = vec![1.25_f64; ROW_SIZE * (HEIGHT + 2)];
            for _ in 0..10_000 {
                for i in 1..=WIDTH {
                    x[i] = x[i + ROW_SIZE];
                    x[i + (HEIGHT + 1) * ROW_SIZE] = x[i + HEIGHT * ROW_SIZE];
                }
                for j in 1..=HEIGHT {
                    x[j * ROW_SIZE] = x[1 + j * ROW_SIZE];
                    x[WIDTH + 1 + j * ROW_SIZE] = x[WIDTH + j * ROW_SIZE];
                }
                for i in 1..=WIDTH {
                    x[i] = -x[i + ROW_SIZE];
                    x[i + (HEIGHT + 1) * ROW_SIZE] = -x[i + HEIGHT * ROW_SIZE];
                }
                black_box(&x);
            }
            black_box(x[1] + x[ROW_SIZE]).to_bits()
        }
        "packed-smi-am3" => {
            const LIMBS: usize = 256;
            let input = vec![1_234_567_f64; LIMBS];
            let mut output = vec![7_654_321_f64; LIMBS];
            let x = 0x1234567_i32;
            let (x_low, x_high) = (x & 0x3fff, x >> 14);
            let mut checksum = 0_i32;
            for round in 0_i32..100_000 {
                let mut carry = round & 255;
                for index in 0..LIMBS {
                    let input = input[index] as i32;
                    let (low, high) = (input & 0x3fff, input >> 14);
                    let product = x_high * low + high * x_low;
                    let value =
                        x_low * low + ((product & 0x3fff) << 14) + output[index] as i32 + carry;
                    carry = (value >> 28) + (product >> 14) + x_high * high;
                    output[index] = f64::from(value & 0xfffffff);
                }
                checksum ^= carry;
            }
            black_box(checksum as u64 ^ output[LIMBS - 1].to_bits())
        }
        "packed-smi-square" => {
            const LIMBS: usize = 128;
            let input = vec![1_234_567_i32; LIMBS];
            let mut output = vec![0_i32; 2 * LIMBS];
            for _ in 0..2_000 {
                output.fill(0);
                for i in 0..LIMBS - 1 {
                    let multiply = |input_index: usize,
                                    output_index: usize,
                                    multiplier: i32,
                                    mut carry: i32,
                                    count: usize,
                                    output: &mut [i32]| {
                        let (x_low, x_high) = (multiplier & 0x3fff, multiplier >> 14);
                        for offset in 0..count {
                            let input = input[input_index + offset];
                            let (low, high) = (input & 0x3fff, input >> 14);
                            let product = x_high * low + high * x_low;
                            let value = x_low * low
                                + ((product & 0x3fff) << 14)
                                + output[output_index + offset]
                                + carry;
                            carry = (value >> 28) + (product >> 14) + x_high * high;
                            output[output_index + offset] = value & 0x0fff_ffff;
                        }
                        carry
                    };
                    let carry = multiply(i, 2 * i, input[i], 0, 1, &mut output);
                    let carry = multiply(
                        i + 1,
                        2 * i + 1,
                        input[i].wrapping_mul(2),
                        carry,
                        LIMBS - i - 1,
                        &mut output,
                    );
                    let index = i + LIMBS;
                    let sum = output[index] + carry;
                    if sum < 0x10000000 {
                        output[index] = sum;
                    } else {
                        output[index] = sum - 0x10000000;
                        output[index + 1] = 1;
                    }
                }
            }
            black_box(output[0] as u64 ^ (output[2 * LIMBS - 2] as u64).rotate_left(17))
        }
        _ => panic!("unknown L0 oracle: {id}"),
    }
}

fn main() {
    let id = env::args().nth(1).expect("missing L0 oracle id");
    println!("{}", black_box(run(&id)));
}
