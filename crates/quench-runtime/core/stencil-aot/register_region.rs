// Mixed Word32/F64 register-region stencil templates.
//
// Every leaf has exactly the same internal ABI.  The template body changes
// one component of that state and must-tail-calls the next template.  Rustc
// therefore performs instruction selection once, while the runtime only
// copies bytes and patches the continuation holes.

const REGISTER_REGION_WORD_LANE_COUNT: usize = 4;
const REGISTER_REGION_F64_LANE_COUNT: usize = 4;
const REGISTER_REGION_COMPARE_INSTRUCTION_COUNT: usize = 2;

type RegisterWords = [u32; REGISTER_REGION_WORD_LANE_COUNT];
type RegisterNumbers = [f64; REGISTER_REGION_F64_LANE_COUNT];

macro_rules! register_region_continue {
    ($hole:ident, $frame:ident, $site:expr, $words:ident, $numbers:ident) => {
        unsafe {
            become $hole(
                $frame,
                $site,
                $words[0],
                $words[1],
                $words[2],
                $words[3],
                $numbers[0],
                $numbers[1],
                $numbers[2],
                $numbers[3],
            )
        }
    };
}

macro_rules! register_region_next {
    ($frame:ident, $site:ident, $words:ident, $numbers:ident) => {{
        let next_site = unsafe {
            $site
                .cast::<u8>()
                .add(site_holes::NEXT_SITE_BYTE_OFFSET_HOLE)
                .cast::<InlineSite>()
        };
        register_region_continue!(
            __quench_register_region_hole_next,
            $frame,
            next_site,
            $words,
            $numbers
        )
    }};
}

/// Defines a leaf in the one fixed register-region category object.  Arrays
/// are only a quoted source-level view: constant indices disappear in the AOT
/// object and the values remain in the ABI's physical registers.
macro_rules! define_register_region_leaf {
    ($name:ident, |$frame:ident, $site:ident, $words:ident, $numbers:ident| $body:block) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            $frame: *mut RawDynFrame,
            $site: *const InlineSite,
            word0: u32,
            word1: u32,
            word2: u32,
            word3: u32,
            number0: f64,
            number1: f64,
            number2: f64,
            number3: f64,
        ) {
            let mut $words: RegisterWords = [word0, word1, word2, word3];
            let mut $numbers: RegisterNumbers = [number0, number1, number2, number3];
            $body
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_register_region_enter(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
    _word0: u32,
    _word1: u32,
    _word2: u32,
    _word3: u32,
    _number0: f64,
    _number1: f64,
    _number2: f64,
    _number3: f64,
) {
    let words: RegisterWords = [0; REGISTER_REGION_WORD_LANE_COUNT];
    let numbers: RegisterNumbers = [0.0; REGISTER_REGION_F64_LANE_COUNT];
    register_region_continue!(
        __quench_register_region_hole_next,
        frame,
        site,
        words,
        numbers
    );
}

define_register_region_leaf!(quench_register_region_leave, |frame, site, words, numbers| {
    register_region_continue!(
        __quench_register_region_hole_leave,
        frame,
        site,
        words,
        numbers
    );
});

define_register_region_leaf!(quench_register_region_nop, |frame, site, words, numbers| {
    register_region_next!(frame, site, words, numbers);
});

define_register_region_leaf!(quench_register_region_jump, |frame, site, words, numbers| {
    let item = unsafe { &*site };
    let target_site = unsafe { (*frame).sites.add(item.literal as usize) };
    register_region_continue!(
        __quench_register_region_hole_next,
        frame,
        target_site,
        words,
        numbers
    );
});

define_register_region_leaf!(quench_register_region_copy_load_local, |frame, site, words, numbers| {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let value = unsafe { *frame_ref.local_values.add(item.left) };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    register_region_next!(frame, site, words, numbers);
});

define_register_region_leaf!(quench_register_region_copy_store_local, |frame, site, words, numbers| {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let value = unsafe { *frame_ref.register_values.add(item.left) };
    unsafe { *frame_ref.local_values.add(item.dst) = value };
    register_region_next!(frame, site, words, numbers);
});

define_register_region_leaf!(quench_register_region_copy_load_name, |frame, site, words, numbers| {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let value = unsafe { *frame_ref.name_snapshots.add(item.literal as usize) };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    register_region_next!(frame, site, words, numbers);
});

define_register_region_leaf!(quench_register_region_copy_move, |frame, site, words, numbers| {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let value = unsafe { *frame_ref.register_values.add(item.left) };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    register_region_next!(frame, site, words, numbers);
});

define_register_region_leaf!(quench_register_region_copy_read_static, |frame, site, words, numbers| {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    unsafe { *frame_ref.register_values.add(item.dst) = *view.elements };
    register_region_next!(frame, site, words, numbers);
});

define_register_region_leaf!(quench_register_region_copy_write_static, |frame, site, words, numbers| {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    unsafe { *view.elements = *frame_ref.register_values.add(item.dst) };
    register_region_next!(frame, site, words, numbers);
});

macro_rules! define_f64_load_family {
    ($definition:ident; $(($name:ident, $lane:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                numbers[$lane] = $definition(frame, site);
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

#[inline(always)]
fn register_region_local_number(frame: *mut RawDynFrame, site: *const InlineSite) -> f64 {
    let item = unsafe { &*site };
    let raw = unsafe { *(*frame).local_values.add(item.left) };
    f64::from_bits(raw.0)
}

#[inline(always)]
fn register_region_literal_number(_frame: *mut RawDynFrame, site: *const InlineSite) -> f64 {
    f64::from_bits(unsafe { (*site).literal })
}

#[inline(always)]
fn register_region_name_number(frame: *mut RawDynFrame, site: *const InlineSite) -> f64 {
    let snapshot = unsafe { (*site).literal as usize };
    let raw = unsafe { *(*frame).name_snapshots.add(snapshot) };
    f64::from_bits(raw.0)
}

#[inline(always)]
fn register_region_static_number(frame: *mut RawDynFrame, site: *const InlineSite) -> f64 {
    let view = unsafe { *(*frame).region_arrays.add((*site).literal as usize) };
    f64::from_bits(unsafe { (*view.elements).0 })
}

define_f64_load_family!(register_region_local_number;
    (quench_register_region_load_local_d0, 0),
    (quench_register_region_load_local_d1, 1),
    (quench_register_region_load_local_d2, 2),
    (quench_register_region_load_local_d3, 3),
);

macro_rules! define_word_local_family {
    ($(($name:ident, $lane:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                let item = unsafe { &*site };
                let raw = unsafe { *(*frame).local_values.add(item.left) };
                words[$lane] = js_u32(f64::from_bits(raw.0));
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

define_word_local_family!(
    (quench_register_region_load_word_local_w0, 0),
    (quench_register_region_load_word_local_w1, 1),
    (quench_register_region_load_word_local_w2, 2),
    (quench_register_region_load_word_local_w3, 3),
);
define_f64_load_family!(register_region_literal_number;
    (quench_register_region_load_literal_d0, 0),
    (quench_register_region_load_literal_d1, 1),
    (quench_register_region_load_literal_d2, 2),
    (quench_register_region_load_literal_d3, 3),
);

macro_rules! define_word_literal_family {
    ($(($name:ident, $lane:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                words[$lane] = raw_value_holes::RAW_VALUE_HOLE_BITS as u32;
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

define_word_literal_family!(
    (quench_register_region_load_word_literal_w0, 0),
    (quench_register_region_load_word_literal_w1, 1),
    (quench_register_region_load_word_literal_w2, 2),
    (quench_register_region_load_word_literal_w3, 3),
);
define_f64_load_family!(register_region_name_number;
    (quench_register_region_load_name_d0, 0),
    (quench_register_region_load_name_d1, 1),
    (quench_register_region_load_name_d2, 2),
    (quench_register_region_load_name_d3, 3),
);
define_f64_load_family!(register_region_static_number;
    (quench_register_region_load_static_d0, 0),
    (quench_register_region_load_static_d1, 1),
    (quench_register_region_load_static_d2, 2),
    (quench_register_region_load_static_d3, 3),
);

macro_rules! define_f64_store_family {
    ($function:ident; $(($name:ident, $lane:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                $function(frame, site, numbers[$lane]);
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

#[inline(always)]
fn register_region_store_local_number(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
    number: f64,
) {
    let item = unsafe { &*site };
    unsafe { *(*frame).local_values.add(item.dst) = canonical_number(number) };
}

#[inline(always)]
fn register_region_write_static_number(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
    number: f64,
) {
    let view = unsafe { *(*frame).region_arrays.add((*site).literal as usize) };
    unsafe { *view.elements = canonical_number(number) };
}

define_f64_store_family!(register_region_store_local_number;
    (quench_register_region_store_local_s0, 0),
    (quench_register_region_store_local_s1, 1),
    (quench_register_region_store_local_s2, 2),
    (quench_register_region_store_local_s3, 3),
);
define_f64_store_family!(register_region_write_static_number;
    (quench_register_region_write_static_s0, 0),
    (quench_register_region_write_static_s1, 1),
    (quench_register_region_write_static_s2, 2),
    (quench_register_region_write_static_s3, 3),
);

macro_rules! define_f64_move_family {
    ($(($name:ident, $destination:expr, $source:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                numbers[$destination] = numbers[$source];
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

macro_rules! define_word_move_family {
    ($(($name:ident, $destination:expr, $source:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                words[$destination] = words[$source];
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

define_f64_move_family!(
    (quench_register_region_move_f64_d00, 0, 0), (quench_register_region_move_f64_d01, 0, 1),
    (quench_register_region_move_f64_d02, 0, 2), (quench_register_region_move_f64_d03, 0, 3),
    (quench_register_region_move_f64_d10, 1, 0), (quench_register_region_move_f64_d11, 1, 1),
    (quench_register_region_move_f64_d12, 1, 2), (quench_register_region_move_f64_d13, 1, 3),
    (quench_register_region_move_f64_d20, 2, 0), (quench_register_region_move_f64_d21, 2, 1),
    (quench_register_region_move_f64_d22, 2, 2), (quench_register_region_move_f64_d23, 2, 3),
    (quench_register_region_move_f64_d30, 3, 0), (quench_register_region_move_f64_d31, 3, 1),
    (quench_register_region_move_f64_d32, 3, 2), (quench_register_region_move_f64_d33, 3, 3),
);
define_word_move_family!(
    (quench_register_region_move_word_w00, 0, 0), (quench_register_region_move_word_w01, 0, 1),
    (quench_register_region_move_word_w02, 0, 2), (quench_register_region_move_word_w03, 0, 3),
    (quench_register_region_move_word_w10, 1, 0), (quench_register_region_move_word_w11, 1, 1),
    (quench_register_region_move_word_w12, 1, 2), (quench_register_region_move_word_w13, 1, 3),
    (quench_register_region_move_word_w20, 2, 0), (quench_register_region_move_word_w21, 2, 1),
    (quench_register_region_move_word_w22, 2, 2), (quench_register_region_move_word_w23, 2, 3),
    (quench_register_region_move_word_w30, 3, 0), (quench_register_region_move_word_w31, 3, 1),
    (quench_register_region_move_word_w32, 3, 2), (quench_register_region_move_word_w33, 3, 3),
);

macro_rules! define_f64_to_word_family {
    ($(($name:ident, $destination:expr, $source:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                words[$destination] = js_u32(numbers[$source]);
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

define_f64_to_word_family!(
    (quench_register_region_f64_to_word_w0d0, 0, 0), (quench_register_region_f64_to_word_w0d1, 0, 1),
    (quench_register_region_f64_to_word_w0d2, 0, 2), (quench_register_region_f64_to_word_w0d3, 0, 3),
    (quench_register_region_f64_to_word_w1d0, 1, 0), (quench_register_region_f64_to_word_w1d1, 1, 1),
    (quench_register_region_f64_to_word_w1d2, 1, 2), (quench_register_region_f64_to_word_w1d3, 1, 3),
    (quench_register_region_f64_to_word_w2d0, 2, 0), (quench_register_region_f64_to_word_w2d1, 2, 1),
    (quench_register_region_f64_to_word_w2d2, 2, 2), (quench_register_region_f64_to_word_w2d3, 2, 3),
    (quench_register_region_f64_to_word_w3d0, 3, 0), (quench_register_region_f64_to_word_w3d1, 3, 1),
    (quench_register_region_f64_to_word_w3d2, 3, 2), (quench_register_region_f64_to_word_w3d3, 3, 3),
);

macro_rules! signed_word_to_f64 { ($value:expr) => { ($value as i32) as f64 }; }
macro_rules! unsigned_word_to_f64 { ($value:expr) => { $value as f64 }; }

macro_rules! define_word_to_f64_family {
    ($conversion:ident; $(($name:ident, $destination:expr, $source:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                numbers[$destination] = $conversion!(words[$source]);
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

define_word_to_f64_family!(signed_word_to_f64;
    (quench_register_region_signed_word_to_f64_d0w0, 0, 0), (quench_register_region_signed_word_to_f64_d0w1, 0, 1),
    (quench_register_region_signed_word_to_f64_d0w2, 0, 2), (quench_register_region_signed_word_to_f64_d0w3, 0, 3),
    (quench_register_region_signed_word_to_f64_d1w0, 1, 0), (quench_register_region_signed_word_to_f64_d1w1, 1, 1),
    (quench_register_region_signed_word_to_f64_d1w2, 1, 2), (quench_register_region_signed_word_to_f64_d1w3, 1, 3),
    (quench_register_region_signed_word_to_f64_d2w0, 2, 0), (quench_register_region_signed_word_to_f64_d2w1, 2, 1),
    (quench_register_region_signed_word_to_f64_d2w2, 2, 2), (quench_register_region_signed_word_to_f64_d2w3, 2, 3),
    (quench_register_region_signed_word_to_f64_d3w0, 3, 0), (quench_register_region_signed_word_to_f64_d3w1, 3, 1),
    (quench_register_region_signed_word_to_f64_d3w2, 3, 2), (quench_register_region_signed_word_to_f64_d3w3, 3, 3),
);
define_word_to_f64_family!(unsigned_word_to_f64;
    (quench_register_region_unsigned_word_to_f64_d0w0, 0, 0), (quench_register_region_unsigned_word_to_f64_d0w1, 0, 1),
    (quench_register_region_unsigned_word_to_f64_d0w2, 0, 2), (quench_register_region_unsigned_word_to_f64_d0w3, 0, 3),
    (quench_register_region_unsigned_word_to_f64_d1w0, 1, 0), (quench_register_region_unsigned_word_to_f64_d1w1, 1, 1),
    (quench_register_region_unsigned_word_to_f64_d1w2, 1, 2), (quench_register_region_unsigned_word_to_f64_d1w3, 1, 3),
    (quench_register_region_unsigned_word_to_f64_d2w0, 2, 0), (quench_register_region_unsigned_word_to_f64_d2w1, 2, 1),
    (quench_register_region_unsigned_word_to_f64_d2w2, 2, 2), (quench_register_region_unsigned_word_to_f64_d2w3, 2, 3),
    (quench_register_region_unsigned_word_to_f64_d3w0, 3, 0), (quench_register_region_unsigned_word_to_f64_d3w1, 3, 1),
    (quench_register_region_unsigned_word_to_f64_d3w2, 3, 2), (quench_register_region_unsigned_word_to_f64_d3w3, 3, 3),
);

macro_rules! define_f64_unary_family {
    ($operation:expr; $(($name:ident, $destination:expr, $source:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                numbers[$destination] = $operation(numbers[$source]);
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

define_f64_unary_family!(|value: f64| value;
    (quench_register_region_unary_plus_d00, 0, 0), (quench_register_region_unary_plus_d01, 0, 1),
    (quench_register_region_unary_plus_d02, 0, 2), (quench_register_region_unary_plus_d03, 0, 3),
    (quench_register_region_unary_plus_d10, 1, 0), (quench_register_region_unary_plus_d11, 1, 1),
    (quench_register_region_unary_plus_d12, 1, 2), (quench_register_region_unary_plus_d13, 1, 3),
    (quench_register_region_unary_plus_d20, 2, 0), (quench_register_region_unary_plus_d21, 2, 1),
    (quench_register_region_unary_plus_d22, 2, 2), (quench_register_region_unary_plus_d23, 2, 3),
    (quench_register_region_unary_plus_d30, 3, 0), (quench_register_region_unary_plus_d31, 3, 1),
    (quench_register_region_unary_plus_d32, 3, 2), (quench_register_region_unary_plus_d33, 3, 3),
);
define_f64_unary_family!(|value: f64| -value;
    (quench_register_region_negate_d00, 0, 0), (quench_register_region_negate_d01, 0, 1),
    (quench_register_region_negate_d02, 0, 2), (quench_register_region_negate_d03, 0, 3),
    (quench_register_region_negate_d10, 1, 0), (quench_register_region_negate_d11, 1, 1),
    (quench_register_region_negate_d12, 1, 2), (quench_register_region_negate_d13, 1, 3),
    (quench_register_region_negate_d20, 2, 0), (quench_register_region_negate_d21, 2, 1),
    (quench_register_region_negate_d22, 2, 2), (quench_register_region_negate_d23, 2, 3),
    (quench_register_region_negate_d30, 3, 0), (quench_register_region_negate_d31, 3, 1),
    (quench_register_region_negate_d32, 3, 2), (quench_register_region_negate_d33, 3, 3),
);

macro_rules! define_word_unary_family {
    ($(($name:ident, $destination:expr, $source:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                words[$destination] = !words[$source];
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

define_word_unary_family!(
    (quench_register_region_bit_not_w00, 0, 0), (quench_register_region_bit_not_w01, 0, 1),
    (quench_register_region_bit_not_w02, 0, 2), (quench_register_region_bit_not_w03, 0, 3),
    (quench_register_region_bit_not_w10, 1, 0), (quench_register_region_bit_not_w11, 1, 1),
    (quench_register_region_bit_not_w12, 1, 2), (quench_register_region_bit_not_w13, 1, 3),
    (quench_register_region_bit_not_w20, 2, 0), (quench_register_region_bit_not_w21, 2, 1),
    (quench_register_region_bit_not_w22, 2, 2), (quench_register_region_bit_not_w23, 2, 3),
    (quench_register_region_bit_not_w30, 3, 0), (quench_register_region_bit_not_w31, 3, 1),
    (quench_register_region_bit_not_w32, 3, 2), (quench_register_region_bit_not_w33, 3, 3),
);

// Rust macros cannot concatenate identifiers, so each operation supplies its
// generated symbol names.  The matrix macro below is intentionally data, not
// 64 hand-written implementations.
macro_rules! define_f64_binary_named {
    ($operator:tt; $(($name:ident, $destination:expr, $left:expr, $right:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                numbers[$destination] = numbers[$left] $operator numbers[$right];
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

macro_rules! f64_binary_family {
    ($operator:tt; $($name:ident),+ $(,)?) => {
        f64_binary_family!(@indices $operator; [$($name),+];
            (0,0,0),(0,0,1),(0,0,2),(0,0,3),(0,1,0),(0,1,1),(0,1,2),(0,1,3),
            (0,2,0),(0,2,1),(0,2,2),(0,2,3),(0,3,0),(0,3,1),(0,3,2),(0,3,3),
            (1,0,0),(1,0,1),(1,0,2),(1,0,3),(1,1,0),(1,1,1),(1,1,2),(1,1,3),
            (1,2,0),(1,2,1),(1,2,2),(1,2,3),(1,3,0),(1,3,1),(1,3,2),(1,3,3),
            (2,0,0),(2,0,1),(2,0,2),(2,0,3),(2,1,0),(2,1,1),(2,1,2),(2,1,3),
            (2,2,0),(2,2,1),(2,2,2),(2,2,3),(2,3,0),(2,3,1),(2,3,2),(2,3,3),
            (3,0,0),(3,0,1),(3,0,2),(3,0,3),(3,1,0),(3,1,1),(3,1,2),(3,1,3),
            (3,2,0),(3,2,1),(3,2,2),(3,2,3),(3,3,0),(3,3,1),(3,3,2),(3,3,3));
    };
    (@indices $operator:tt; [$head:ident $(,$tail:ident)*];
     ($destination:expr,$left:expr,$right:expr) $(,$indices:tt)*) => {
        define_f64_binary_named!($operator; ($head, $destination, $left, $right));
        f64_binary_family!(@indices $operator; [$($tail),*]; $($indices),*);
    };
    (@indices $operator:tt; []; ) => {};
}

f64_binary_family!(+;
    quench_register_region_add_d000,quench_register_region_add_d001,quench_register_region_add_d002,quench_register_region_add_d003,
    quench_register_region_add_d010,quench_register_region_add_d011,quench_register_region_add_d012,quench_register_region_add_d013,
    quench_register_region_add_d020,quench_register_region_add_d021,quench_register_region_add_d022,quench_register_region_add_d023,
    quench_register_region_add_d030,quench_register_region_add_d031,quench_register_region_add_d032,quench_register_region_add_d033,
    quench_register_region_add_d100,quench_register_region_add_d101,quench_register_region_add_d102,quench_register_region_add_d103,
    quench_register_region_add_d110,quench_register_region_add_d111,quench_register_region_add_d112,quench_register_region_add_d113,
    quench_register_region_add_d120,quench_register_region_add_d121,quench_register_region_add_d122,quench_register_region_add_d123,
    quench_register_region_add_d130,quench_register_region_add_d131,quench_register_region_add_d132,quench_register_region_add_d133,
    quench_register_region_add_d200,quench_register_region_add_d201,quench_register_region_add_d202,quench_register_region_add_d203,
    quench_register_region_add_d210,quench_register_region_add_d211,quench_register_region_add_d212,quench_register_region_add_d213,
    quench_register_region_add_d220,quench_register_region_add_d221,quench_register_region_add_d222,quench_register_region_add_d223,
    quench_register_region_add_d230,quench_register_region_add_d231,quench_register_region_add_d232,quench_register_region_add_d233,
    quench_register_region_add_d300,quench_register_region_add_d301,quench_register_region_add_d302,quench_register_region_add_d303,
    quench_register_region_add_d310,quench_register_region_add_d311,quench_register_region_add_d312,quench_register_region_add_d313,
    quench_register_region_add_d320,quench_register_region_add_d321,quench_register_region_add_d322,quench_register_region_add_d323,
    quench_register_region_add_d330,quench_register_region_add_d331,quench_register_region_add_d332,quench_register_region_add_d333,
);

macro_rules! define_word_binary_named {
    ($operator:tt; $(($name:ident, $destination:expr, $left:expr, $right:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                words[$destination] = words[$left] $operator words[$right];
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

macro_rules! register_region_shift {
    (left, $value:expr, $count:expr) => { $value.wrapping_shl($count) };
    (right, $value:expr, $count:expr) => { (($value as i32).wrapping_shr($count)) as u32 };
    (right_unsigned, $value:expr, $count:expr) => { $value.wrapping_shr($count) };
}

macro_rules! define_word_shift_named {
    ($direction:ident; $(($name:ident, $destination:expr, $left:expr, $right:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                let count = words[$right] & SHIFT_COUNT_MASK;
                words[$destination] = register_region_shift!($direction, words[$left], count);
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

macro_rules! define_f64_compare_named {
    ($operator:tt; $(($name:ident, $left:expr, $right:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                let result = numbers[$left] $operator numbers[$right];
                if result {
                    let next_site = unsafe { site.add(REGISTER_REGION_COMPARE_INSTRUCTION_COUNT) };
                    register_region_continue!(
                        __quench_register_region_hole_next,
                        frame,
                        next_site,
                        words,
                        numbers
                    );
                }
                let branch = unsafe { &*site.add(1) };
                let target_site = unsafe { (*frame).sites.add(branch.literal as usize) };
                register_region_continue!(
                    __quench_register_region_hole_branch,
                    frame,
                    target_site,
                    words,
                    numbers
                );
            });
        )+
    };
}

#[inline(always)]
fn register_region_dense_index(number: f64) -> Option<usize> {
    if !number.is_finite() || number < 0.0 || number > MAX_JS_ARRAY_INDEX {
        return None;
    }
    let index = number as usize;
    ((index as f64) == number).then_some(index)
}

#[inline(always)]
fn register_region_proven_dense_index(number: f64) -> Option<usize> {
    (number <= MAX_JS_ARRAY_INDEX).then_some(number as usize)
}

macro_rules! define_dense_read_named {
    ($index_function:ident; $(($name:ident, $destination:expr, $index:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                let item = unsafe { &*site };
                let view = unsafe { *(*frame).region_arrays.add(item.literal as usize) };
                let index = $index_function(numbers[$index])
                    .filter(|index| *index < view.length);
                let Some(index) = index else {
                    unsafe {
                        *(*frame).register_values.add(item.right) = canonical_number(numbers[$index]);
                    }
                    register_region_continue!(
                        __quench_register_region_hole_slow,
                        frame,
                        site,
                        words,
                        numbers
                    );
                };
                numbers[$destination] = f64::from_bits(unsafe { (*view.elements.add(index)).0 });
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

macro_rules! define_dense_write_named {
    ($index_function:ident; $(($name:ident, $index:expr, $source:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                let item = unsafe { &*site };
                let view = unsafe { *(*frame).region_arrays.add(item.literal as usize) };
                let index = $index_function(numbers[$index])
                    .filter(|index| *index < view.length);
                let Some(index) = index else {
                    unsafe {
                        *(*frame).register_values.add(item.right) = canonical_number(numbers[$index]);
                        *(*frame).register_values.add(item.dst) = canonical_number(numbers[$source]);
                    }
                    register_region_continue!(
                        __quench_register_region_hole_slow,
                        frame,
                        site,
                        words,
                        numbers
                    );
                };
                unsafe { *view.elements.add(index) = canonical_number(numbers[$source]) };
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

macro_rules! define_dense_word_read_named {
    ($index_function:ident; $(($name:ident, $destination:expr, $index:expr)),+ $(,)?) => {
        $(
            define_register_region_leaf!($name, |frame, site, words, numbers| {
                let item = unsafe { &*site };
                let view = unsafe { *(*frame).region_arrays.add(item.literal as usize) };
                let index = $index_function(numbers[$index])
                    .filter(|index| *index < view.length);
                let Some(index) = index else {
                    unsafe {
                        *(*frame).register_values.add(item.right) = canonical_number(numbers[$index]);
                    }
                    register_region_continue!(
                        __quench_register_region_hole_slow,
                        frame,
                        site,
                        words,
                        numbers
                    );
                };
                let raw = unsafe { *view.elements.add(index) };
                words[$destination] = js_u32(f64::from_bits(raw.0));
                register_region_next!(frame, site, words, numbers);
            });
        )+
    };
}

// Build-time expansion enumerates the finite physical-register matrices while
// all semantics remain in the macros above.
include!(concat!(env!("OUT_DIR"), "/register_region_generated.rs"));
