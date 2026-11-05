use super::{ArrayElements, Bencher, Value, black_box};
use std::sync::OnceLock;

#[global_allocator]
static ALLOCATOR: quench_alloc::QuenchAlloc = quench_alloc::QuenchAlloc::new();

// One route per process. Initialize before constructing fixtures or entering timing.
// The disabled route still includes QuenchAlloc's forwarding checks.
fn allocator_route() -> bool {
    static QUENCH: OnceLock<bool> = OnceLock::new();
    *QUENCH.get_or_init(|| {
        let enabled = match std::env::var("QUENCH_GROWTH_ALLOCATOR").as_deref() {
            Ok("system") => false,
            Ok("quench") => true,
            _ => panic!("set QUENCH_GROWTH_ALLOCATOR to system or quench"),
        };
        if enabled { quench_alloc::QuenchAlloc::enable(); }
        let probe = ArrayElements::from_slice(&[Value::NULL]);
        assert_eq!(quench_alloc::owns(probe.as_ptr().cast()), enabled);
        eprintln!("growth allocator: {}", if enabled { "quench" } else { "system forwarding" });
        enabled
    })
}

fn append<const PUSH: bool>(unique: &mut crate::heap::UniqueElements, value: Value) {
    if PUSH { unique.push(value); }
    else { unique.extend_from_slice(std::slice::from_ref(&value)); }
}

fn build<const PUSH: bool>(values: &[Value]) -> ArrayElements {
    let mut elements = ArrayElements::new();
    let unique = elements.make_mut();
    for value in values {
        append::<PUSH>(unique, *value);
    }
    elements
}

fn grow<const PUSH: bool>(b: &mut Bencher, length: usize) {
    allocator_route();
    let values: Vec<_> = (0..length).map(|i| {
        if i % 2 == 0 { Value::integer(i as i32) } else { Value::NULL }
    }).collect();
    let validate = || {
        let elements = build::<PUSH>(&values);
        assert_eq!(elements.as_slice(), values);
        assert_eq!(elements.owner_count(), 1);
        assert!(!elements.holds_only_int32());
        assert!(elements.capacity() >= length);
    };
    validate();
    // One complete grow-and-drop lifecycle. Input construction and checks are excluded;
    // allocation, every append, moving copies and final deallocation are included.
    b.iter(|| black_box(build::<PUSH>(black_box(&values))));
    validate();
}

// Fixed experimental lengths span small backing classes and the system boundary;
// they deliberately do not change when the production growth policy changes.
macro_rules! growth_cases {
    ($($name:ident, $length:literal, $push:literal;)*) => {$(
        #[bench]
        fn $name(b: &mut Bencher) { grow::<$push>(b, $length); }
    )*};
}
growth_cases! {
    primitive_array_growth_push_16, 16, true;
    primitive_array_growth_extend_one_16, 16, false;
    primitive_array_growth_push_100, 100, true;
    primitive_array_growth_extend_one_100, 100, false;
    primitive_array_growth_push_128, 128, true;
    primitive_array_growth_extend_one_128, 128, false;
    primitive_array_growth_push_1024, 1024, true;
    primitive_array_growth_extend_one_1024, 1024, false;
}

#[test]
fn primitive_array_growth_transitions() {
    fn trace<const PUSH: bool>() {
        let enabled = allocator_route();
        let mut elements = ArrayElements::new();
        for index in 0..1024 {
            let before = (elements.as_ptr(), elements.capacity());
            append::<PUSH>(elements.make_mut(), Value::integer(index));
            if elements.capacity() != before.1 {
                let bytes = elements.allocated_bytes();
                let owned = quench_alloc::owns(elements.as_ptr().cast());
                assert_eq!(owned, enabled && quench_alloc::class_of(bytes).is_some());
                eprintln!("push={PUSH} len={} capacity={} bytes={bytes} quench_owned={owned} moved={}",
                    elements.len(), elements.capacity(), elements.as_ptr() != before.0);
            }
        }
        assert!(elements.holds_only_int32());
        for (index, value) in elements.iter().enumerate() {
            assert_eq!(*value, Value::integer(index as i32));
        }
    }
    trace::<true>();
    trace::<false>();
}
