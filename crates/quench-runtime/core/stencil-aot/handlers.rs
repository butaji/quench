#![feature(explicit_tail_calls)]
#![no_std]

use core::sync::atomic::{AtomicU64, Ordering};

mod operand_holes {
    include!("operand_holes.rs");
}

mod site_holes {
    include!("site_holes.rs");
}

mod raw_value_holes {
    include!("raw_value_holes.rs");
}

mod object_layout {
    include!("object_layout.rs");
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RawValue(pub u64);

unsafe extern "C" {
    fn __quench_hole_next(
        thread: *mut u8,
        frame: *mut RawValue,
        code: *const u8,
        site: *mut u8,
        accumulator: RawValue,
        tag_mask: u64,
        first_tag: u64,
        scratch: u64,
    ) -> RawValue;

    fn __quench_dyn_hole_next(frame: *mut RawDynFrame, site: *const InlineSite);
    fn __quench_dyn_hole_slow(frame: *mut RawDynFrame, site: *const InlineSite);
    fn __quench_dyn_hole_branch(frame: *mut RawDynFrame, site: *const InlineSite);
    fn __quench_register_region_hole_next(
        frame: *mut RawDynFrame,
        site: *const InlineSite,
        word0: u32,
        word1: u32,
        word2: u32,
        word3: u32,
        number0: f64,
        number1: f64,
        number2: f64,
        number3: f64,
    );
    fn __quench_register_region_hole_branch(
        frame: *mut RawDynFrame,
        site: *const InlineSite,
        word0: u32,
        word1: u32,
        word2: u32,
        word3: u32,
        number0: f64,
        number1: f64,
        number2: f64,
        number3: f64,
    );
    fn __quench_register_region_hole_slow(
        frame: *mut RawDynFrame,
        site: *const InlineSite,
        word0: u32,
        word1: u32,
        word2: u32,
        word3: u32,
        number0: f64,
        number1: f64,
        number2: f64,
        number3: f64,
    );
    fn __quench_register_region_hole_leave(
        frame: *mut RawDynFrame,
        site: *const InlineSite,
        word0: u32,
        word1: u32,
        word2: u32,
        word3: u32,
        number0: f64,
        number1: f64,
        number2: f64,
        number3: f64,
    );
}

const TAG_MASK: u64 = 0xffff_0000_0000_0000;
const PAYLOAD_MASK: u64 = 0x0000_ffff_ffff_ffff;
const FIRST_TAG: u64 = 0xfff9_0000_0000_0000;
const FIRST_HEAP_TAG: u64 = 0xfffc_0000_0000_0000;
const CANONICAL_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;
const UNDEFINED_TAG: u64 = FIRST_TAG;
const NULL_TAG: u64 = 0xfffa_0000_0000_0000;
const BOOL_TAG: u64 = 0xfffb_0000_0000_0000;
const STRING_TAG: u64 = 0xfffc_0000_0000_0000;
const OBJECT_TAG: u64 = 0xfffd_0000_0000_0000;
const FALSE_PAYLOAD: u64 = 0;
const TRUE_PAYLOAD: u64 = 1;
const TRUTHINESS_FALSE: u8 = 0;
const TRUTHINESS_TRUE: u8 = 1;
const TRUTHINESS_NEEDS_SLOW_PATH: u8 = 2;
const LOOP_ADD_LOAD_LOCAL_INDEX: usize = 0;
const LOOP_ADD_LOAD_LITERAL_INDEX: usize = 1;
const LOOP_ADD_BINARY_INDEX: usize = 2;
const LOOP_ADD_STORE_LOCAL_INDEX: usize = 3;
const LOOP_ADD_JUMP_INDEX: usize = 4;
const CONDITION_LOAD_LOCAL_INDEX: usize = 0;
const CONDITION_RIGHT_OPERAND_SITE_INDEX: usize = 1;
const CONDITION_COMPARE_INDEX: usize = 2;
const CONDITION_BRANCH_INDEX: usize = 3;
const CONDITION_INSTRUCTION_COUNT: usize = 4;
const NAME_CONDITION_LOAD_LOCAL_INDEX: usize = 0;
const NAME_CONDITION_LOAD_NAME_INDEX: usize = 1;
const NAME_CONDITION_BRANCH_INDEX: usize = 3;
const NAME_CONDITION_INSTRUCTION_COUNT: usize = 4;
const DIRECT_CALL_SUCCESS: usize = 0;
const DIRECT_CALL_EXCEPTION: usize = 1;
const GET_STATIC_CALL_GET_INDEX: usize = 0;
const GET_STATIC_CALL_CALL_INDEX: usize = 1;
const GET_STATIC_CALL_INSTRUCTION_COUNT: usize = 2;
const _: () = assert!(GET_STATIC_CALL_GET_INDEX + 1 == GET_STATIC_CALL_CALL_INDEX);
const _: () = assert!(GET_STATIC_CALL_CALL_INDEX + 1 == GET_STATIC_CALL_INSTRUCTION_COUNT);
const GET_STATIC_LOAD_LOCAL_CALL_GET_INDEX: usize = 0;
const GET_STATIC_LOAD_LOCAL_CALL_LOAD_INDEX: usize = 1;
const GET_STATIC_LOAD_LOCAL_CALL_CALL_INDEX: usize = 2;
const GET_STATIC_LOAD_LOCAL_CALL_INSTRUCTION_COUNT: usize = 3;
const _: () = assert!(
    GET_STATIC_LOAD_LOCAL_CALL_GET_INDEX + 1 == GET_STATIC_LOAD_LOCAL_CALL_LOAD_INDEX
);
const _: () = assert!(
    GET_STATIC_LOAD_LOCAL_CALL_LOAD_INDEX + 1 == GET_STATIC_LOAD_LOCAL_CALL_CALL_INDEX
);
const _: () = assert!(
    GET_STATIC_LOAD_LOCAL_CALL_CALL_INDEX + 1 == GET_STATIC_LOAD_LOCAL_CALL_INSTRUCTION_COUNT
);
const UNUSED_SITE_OPERAND: usize = usize::MAX;
const UPDATE_LOAD_LOCAL_INDEX: usize = 0;
const UPDATE_LITERAL_INDEX: usize = 1;
const UPDATE_STORE_LOCAL_INDEX: usize = 3;
const UPDATE_JUMP_INDEX: usize = 4;
const LOCAL_UPDATE_INSTRUCTION_COUNT: usize = 4;
const RECURRENCE_BOUND_LITERAL_INDEX: usize = 4;
const RECURRENCE_BRANCH_INDEX: usize = 6;
const RECURRENCE_INSTRUCTION_COUNT: usize = 7;
const PROPERTY_EQUAL_FIRST_GET_INDEX: usize = 1;
const PROPERTY_EQUAL_SECOND_LOAD_INDEX: usize = 2;
const PROPERTY_EQUAL_SECOND_GET_INDEX: usize = 3;
const PROPERTY_EQUAL_BRANCH_INDEX: usize = 5;
const PROPERTY_EQUAL_INSTRUCTION_COUNT: usize = 6;
const PROPERTY_LITERAL_GET_INDEX: usize = 1;
const PROPERTY_LITERAL_LITERAL_INDEX: usize = 2;
const PROPERTY_LITERAL_BRANCH_INDEX: usize = 4;
const PROPERTY_LITERAL_INSTRUCTION_COUNT: usize = 5;
const PROPERTY_LOAD_GET_INDEX: usize = 1;
const PROPERTY_LOAD_STORE_INDEX: usize = 2;
const PROPERTY_LOAD_JUMP_INDEX: usize = 3;
const PROPERTY_LOAD_JUMP_INSTRUCTION_COUNT: usize = 4;
const _: () = assert!(PROPERTY_LOAD_JUMP_INDEX + 1 == PROPERTY_LOAD_JUMP_INSTRUCTION_COUNT);
const TWO_PROPERTY_STORE_FIRST_RECEIVER_LOAD_INDEX: usize = 0;
const TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_INDEX: usize = 1;
const TWO_PROPERTY_STORE_FIRST_SET_INDEX: usize = 2;
const TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_INDEX: usize = 3;
const TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_INDEX: usize = 4;
const TWO_PROPERTY_STORE_SECOND_SET_INDEX: usize = 5;
const TWO_PROPERTY_STORE_RETURN_INDEX: usize = 6;
const TWO_PROPERTY_STORE_INSTRUCTION_COUNT: usize = 7;
const _: () = assert!(
    TWO_PROPERTY_STORE_FIRST_RECEIVER_LOAD_INDEX + 1 == TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_INDEX
);
const _: () = assert!(
    TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_INDEX + 1 == TWO_PROPERTY_STORE_FIRST_SET_INDEX
);
const _: () = assert!(
    TWO_PROPERTY_STORE_FIRST_SET_INDEX + 1 == TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_INDEX
);
const _: () = assert!(
    TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_INDEX + 1 == TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_INDEX
);
const _: () = assert!(
    TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_INDEX + 1 == TWO_PROPERTY_STORE_SECOND_SET_INDEX
);
const _: () = assert!(
    TWO_PROPERTY_STORE_SECOND_SET_INDEX + 1 == TWO_PROPERTY_STORE_RETURN_INDEX
);
const _: () = assert!(TWO_PROPERTY_STORE_RETURN_INDEX + 1 == TWO_PROPERTY_STORE_INSTRUCTION_COUNT);
const INSTANCEOF_BRANCH_INDEX: usize = 3;
const INSTANCEOF_INSTRUCTION_COUNT: usize = 4;
const INSTANCEOF_NOT_BRANCH_INDEX: usize = 4;
const INSTANCEOF_NOT_INSTRUCTION_COUNT: usize = 5;
const INSTANCEOF_CACHE_FALSE: usize = 0;
const INSTANCEOF_CACHE_TRUE: usize = 1;
const _: () = assert!(INSTANCEOF_BRANCH_INDEX + 1 == INSTANCEOF_INSTRUCTION_COUNT);
const _: () = assert!(INSTANCEOF_NOT_BRANCH_INDEX + 1 == INSTANCEOF_NOT_INSTRUCTION_COUNT);
const CONSTANT_CONDITION_BRANCH_INDEX: usize = 1;
const CONSTANT_CONDITION_INSTRUCTION_COUNT: usize = 2;
const MAX_JS_ARRAY_INDEX: f64 = 4_294_967_294.0;
const SHIFT_COUNT_MASK: u32 = 31;
const F64_EXPONENT_MASK: u64 = 0x7ff;
const F64_EXPONENT_SHIFT: u32 = 52;
const F64_EXPONENT_BIAS: i32 = 1023;
const F64_FRACTION_MASK: u64 = (1_u64 << F64_EXPONENT_SHIFT) - 1;
const F64_IMPLICIT_BIT: u64 = 1_u64 << F64_EXPONENT_SHIFT;
const UINT32_BITS: i32 = 32;
const MAX_U64_SHIFT: i32 = u64::BITS as i32 - 1;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct RawDenseView {
    pub elements: *mut RawValue,
    pub length: usize,
}

const OBJECT_SHAPE_WORD_OFFSET: usize = object_layout::SHAPE_WORD_OFFSET;
const OBJECT_SLOTS_WORD_OFFSET: usize = object_layout::SLOTS_WORD_OFFSET;
const OBJECT_PROTOTYPE_WORD_OFFSET: usize = object_layout::PROTOTYPE_WORD_OFFSET;
const OBJECT_DENSE_ELEMENTS_WORD_OFFSET: usize = object_layout::DENSE_ELEMENTS_WORD_OFFSET;
const OBJECT_DENSE_LENGTH_WORD_OFFSET: usize = object_layout::DENSE_LENGTH_WORD_OFFSET;

include!("guest_frame_schema.rs");

define_guest_frame_abi! {
    pub struct RawDynFrame {
        slot_value: RawValue,
        result_value: RawValue,
        site: InlineSite,
        region_array: RawDenseView,
        callback_owner: RawDynFrame,
        name_ic: RawNameIc,
        environment_access: RawEnvironmentAccess,
    }
}


#[repr(C)]
pub struct InlineSite {
    pub pc: usize,
    pub opcode: usize,
    pub run_end: usize,
    pub dst: usize,
    pub left: usize,
    pub right: usize,
    pub literal: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct RawPropertyIc {
    receiver_shape: *const u8,
    slot: usize,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct RawInheritedPropertyIc {
    receiver_shape: *const u8,
    holder_identity: *const usize,
    holder_shape: *const u8,
    slot: usize,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct RawPropertyIcSite {
    own: RawPropertyIc,
    inherited: RawInheritedPropertyIc,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct RawNameIc {
    depth: usize,
    slot: usize,
    layout: *const u8,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct RawEnvironmentAccess {
    layout: *const u8,
    values: *mut RawValue,
    len: usize,
}

const INLINE_LOAD_LITERAL: usize = 1;
const INLINE_LOAD_LOCAL: usize = 2;
const INLINE_STORE_LOCAL: usize = 3;
const INLINE_MOVE: usize = 4;
const INLINE_ADD: usize = 5;
const INLINE_SUBTRACT: usize = 6;
const INLINE_MULTIPLY: usize = 7;
const INLINE_DIVIDE: usize = 8;
const INLINE_LESS: usize = 10;
const INLINE_LESS_EQUAL: usize = 11;
const INLINE_GREATER: usize = 12;
const INLINE_GREATER_EQUAL: usize = 13;
const INLINE_JUMP: usize = 14;
const INLINE_JUMP_IF_FALSE: usize = 15;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_run(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let run_end = unsafe { (*site).run_end };
    let mut current = site;
    while unsafe { (*current).pc } < run_end {
        let item = unsafe { &*current };
        let mut taken_target = core::ptr::null();
        let succeeded = match item.opcode {
            INLINE_LOAD_LITERAL => {
                let frame_ref = unsafe { &mut *frame };
                let destination = unsafe { frame_ref.register_values.add(item.dst) };
                if needs_reference_count(unsafe { *destination }) {
                    false
                } else {
                    unsafe { *destination = RawValue(item.literal) };
                    true
                }
            }
            INLINE_LOAD_LOCAL => {
                let frame_ref = unsafe { &mut *frame };
                if item.left >= frame_ref.local_count {
                    false
                } else {
                    let source = unsafe { *frame_ref.local_values.add(item.left) };
                    let destination = unsafe { frame_ref.register_values.add(item.dst) };
                    if needs_reference_count(source)
                        || needs_reference_count(unsafe { *destination })
                    {
                        false
                    } else {
                        unsafe { *destination = source };
                        true
                    }
                }
            }
            INLINE_STORE_LOCAL => {
                let frame_ref = unsafe { &mut *frame };
                if item.dst >= frame_ref.local_count {
                    false
                } else {
                    let source = unsafe { *frame_ref.register_values.add(item.left) };
                    let destination = unsafe { frame_ref.local_values.add(item.dst) };
                    if needs_reference_count(source)
                        || needs_reference_count(unsafe { *destination })
                    {
                        false
                    } else {
                        unsafe { *destination = source };
                        true
                    }
                }
            }
            INLINE_MOVE => {
                let frame_ref = unsafe { &mut *frame };
                let source = unsafe { *frame_ref.register_values.add(item.left) };
                let destination = unsafe { frame_ref.register_values.add(item.dst) };
                if needs_reference_count(source)
                    || needs_reference_count(unsafe { *destination })
                {
                    false
                } else {
                    unsafe { *destination = source };
                    true
                }
            }
            INLINE_ADD | INLINE_SUBTRACT | INLINE_MULTIPLY | INLINE_DIVIDE => {
                let frame_ref = unsafe { &mut *frame };
                let left = unsafe { *frame_ref.register_values.add(item.left) };
                let right = unsafe { *frame_ref.register_values.add(item.right) };
                let destination = unsafe { frame_ref.register_values.add(item.dst) };
                if !is_number(left)
                    || !is_number(right)
                    || needs_reference_count(unsafe { *destination })
                {
                    false
                } else {
                    let left = f64::from_bits(left.0);
                    let right = f64::from_bits(right.0);
                    let result = match item.opcode {
                        INLINE_ADD => left + right,
                        INLINE_SUBTRACT => left - right,
                        INLINE_MULTIPLY => left * right,
                        INLINE_DIVIDE => left / right,
                        _ => unsafe { core::hint::unreachable_unchecked() },
                    };
                    unsafe { *destination = canonical_number(result) };
                    true
                }
            }
            INLINE_LESS | INLINE_LESS_EQUAL | INLINE_GREATER | INLINE_GREATER_EQUAL => {
                let frame_ref = unsafe { &mut *frame };
                let left = unsafe { *frame_ref.register_values.add(item.left) };
                let right = unsafe { *frame_ref.register_values.add(item.right) };
                let destination = unsafe { frame_ref.register_values.add(item.dst) };
                if !is_number(left)
                    || !is_number(right)
                    || needs_reference_count(unsafe { *destination })
                {
                    false
                } else {
                    let left = f64::from_bits(left.0);
                    let right = f64::from_bits(right.0);
                    let result = match item.opcode {
                        INLINE_LESS => left < right,
                        INLINE_LESS_EQUAL => left <= right,
                        INLINE_GREATER => left > right,
                        INLINE_GREATER_EQUAL => left >= right,
                        _ => unsafe { core::hint::unreachable_unchecked() },
                    };
                    let payload = if result { TRUE_PAYLOAD } else { FALSE_PAYLOAD };
                    unsafe { *destination = RawValue(BOOL_TAG | payload) };
                    true
                }
            }
            INLINE_JUMP => {
                let target_pc = item.literal as usize;
                let sites = unsafe { core::ptr::addr_of!((*frame).sites).read() };
                taken_target = unsafe { sites.add(target_pc) };
                true
            }
            INLINE_JUMP_IF_FALSE => {
                let registers = unsafe { core::ptr::addr_of!((*frame).register_values).read() };
                let test = unsafe { *registers.add(item.left) };
                match immediate_truthiness(test) {
                    TRUTHINESS_TRUE => true,
                    TRUTHINESS_FALSE => {
                        let sites = unsafe { core::ptr::addr_of!((*frame).sites).read() };
                        taken_target = unsafe { sites.add(item.literal as usize) };
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if !taken_target.is_null() {
            unsafe { become __quench_dyn_hole_branch(frame, taken_target) }
        }
        if !succeeded {
            unsafe { become __quench_dyn_hole_slow(frame, current) }
        }
        current = unsafe { current.add(1) };
    }
    unsafe { become __quench_dyn_hole_next(frame, current) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_observe_entry(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let counter = raw_value_holes::RAW_VALUE_HOLE_BITS as usize as *const AtomicU64;
    unsafe { &*counter }.fetch_add(1, Ordering::Relaxed);
    unsafe { become __quench_dyn_hole_next(frame, site) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_direct_call(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let direct_call = unsafe { (*frame).direct_call };
    match unsafe { direct_call(frame, site) } {
        DIRECT_CALL_SUCCESS => unsafe { become __quench_dyn_hole_next(frame, site.add(1)) },
        DIRECT_CALL_EXCEPTION => unsafe { become __quench_dyn_hole_branch(frame, site) },
        _ => unsafe { become __quench_dyn_hole_slow(frame, site) },
    }
}

/// A coarse method-call stencil. The property slot owns the loaded function for the
/// duration of the call, so the temporary register may borrow its raw word without an
/// Rc increment. Selection proves that the temporary is dead after the call. Every
/// exit clears the borrow; a miss then replays the untouched pair canonically.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_get_static_call(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let get_site = unsafe { &*site.add(GET_STATIC_CALL_GET_INDEX) };
    let call_site = unsafe { site.add(GET_STATIC_CALL_CALL_INDEX) };
    let receiver = unsafe { *frame_ref.register_values.add(get_site.left) };
    if let Some(callee) = unsafe { read_cached_property(receiver, site) } {
        let temporary = unsafe { frame_ref.register_values.add(get_site.dst) };
        let old = unsafe { *temporary };
        let borrows_callee = !needs_reference_count(old);
        if borrows_callee || old.0 == callee.0 {
            if borrows_callee {
                unsafe { *temporary = callee };
            }
            let direct_call = frame_ref.direct_call;
            let outcome = unsafe { direct_call(frame, call_site) };
            if borrows_callee {
                unsafe { *temporary = RawValue(UNDEFINED_TAG) };
            }
            match outcome {
                DIRECT_CALL_SUCCESS => unsafe {
                    become __quench_dyn_hole_next(
                        frame,
                        site.add(GET_STATIC_CALL_INSTRUCTION_COUNT),
                    )
                },
                DIRECT_CALL_EXCEPTION => unsafe {
                    become __quench_dyn_hole_branch(frame, call_site)
                },
                _ => {}
            }
        }
    }
    unsafe { become __quench_dyn_hole_slow(frame, site) }
}

/// The one-argument method-call form emitted by the frontend. Both temporary
/// registers borrow from longer-lived owners (the property slot and local slot), then
/// are cleared together before the continuation or rollback edge.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_get_static_load_local_call(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let get_site = unsafe { &*site.add(GET_STATIC_LOAD_LOCAL_CALL_GET_INDEX) };
    let load_site = unsafe { &*site.add(GET_STATIC_LOAD_LOCAL_CALL_LOAD_INDEX) };
    let call_site = unsafe { site.add(GET_STATIC_LOAD_LOCAL_CALL_CALL_INDEX) };
    let receiver = unsafe { *frame_ref.register_values.add(get_site.left) };
    if let Some(callee) = unsafe { read_cached_property(receiver, site) }
        && load_site.left < frame_ref.local_count
    {
        let argument = unsafe { *frame_ref.local_values.add(load_site.left) };
        let callee_temporary = unsafe { frame_ref.register_values.add(get_site.dst) };
        let argument_temporary = unsafe { frame_ref.register_values.add(load_site.dst) };
        let old_callee = unsafe { *callee_temporary };
        let old_argument = unsafe { *argument_temporary };
        let borrows_callee = !needs_reference_count(old_callee);
        let borrows_argument = !needs_reference_count(old_argument);
        let callee_compatible = borrows_callee || old_callee.0 == callee.0;
        let argument_compatible = borrows_argument || old_argument.0 == argument.0;
        if callee_compatible && argument_compatible {
            if borrows_callee {
                unsafe { *callee_temporary = callee };
            }
            if borrows_argument {
                unsafe { *argument_temporary = argument };
            }
            let direct_call = frame_ref.direct_call;
            let outcome = unsafe { direct_call(frame, call_site) };
            if borrows_callee {
                unsafe { *callee_temporary = RawValue(UNDEFINED_TAG) };
            }
            if borrows_argument {
                unsafe { *argument_temporary = RawValue(UNDEFINED_TAG) };
            }
            match outcome {
                DIRECT_CALL_SUCCESS => unsafe {
                    become __quench_dyn_hole_next(
                        frame,
                        site.add(GET_STATIC_LOAD_LOCAL_CALL_INSTRUCTION_COUNT),
                    )
                },
                DIRECT_CALL_EXCEPTION => unsafe {
                    become __quench_dyn_hole_branch(frame, call_site)
                },
                _ => {}
            }
        }
    }
    unsafe { become __quench_dyn_hole_slow(frame, site) }
}

#[inline(always)]
fn is_number(value: RawValue) -> bool {
    value.0 & TAG_MASK < FIRST_TAG
}

#[inline(always)]
fn needs_reference_count(value: RawValue) -> bool {
    let tag = value.0 & TAG_MASK;
    tag >= STRING_TAG && tag != OBJECT_TAG
}

#[inline(always)]
fn canonical_number(value: f64) -> RawValue {
    if value.is_nan() {
        RawValue(CANONICAL_NAN_BITS)
    } else {
        RawValue(value.to_bits())
    }
}

#[inline(always)]
unsafe fn read_cached_property(
    receiver: RawValue,
    get_site: *const InlineSite,
) -> Option<RawValue> {
    if receiver.0 & TAG_MASK != OBJECT_TAG {
        return None;
    }
    let object = (receiver.0 & PAYLOAD_MASK) as usize as *const usize;
    let cache = unsafe { (*get_site).literal as usize as *const RawPropertyIcSite };
    let actual_shape = unsafe { *object.add(OBJECT_SHAPE_WORD_OFFSET) } as *const u8;
    let own = unsafe { &(*cache).own };
    if actual_shape == own.receiver_shape {
        let slots = unsafe { *object.add(OBJECT_SLOTS_WORD_OFFSET) } as *const RawValue;
        return Some(unsafe { *slots.add(own.slot) });
    }
    let inherited = unsafe { &(*cache).inherited };
    let holder = unsafe { *object.add(OBJECT_PROTOTYPE_WORD_OFFSET) } as *const usize;
    if actual_shape == inherited.receiver_shape && holder == inherited.holder_identity {
        let holder_shape = unsafe { *holder.add(OBJECT_SHAPE_WORD_OFFSET) } as *const u8;
        if holder_shape == inherited.holder_shape {
            let slots = unsafe { *holder.add(OBJECT_SLOTS_WORD_OFFSET) } as *const RawValue;
            return Some(unsafe { *slots.add(inherited.slot) });
        }
    }
    None
}

#[inline(always)]
unsafe fn cached_own_property_slot(
    receiver: RawValue,
    set_site: *const InlineSite,
) -> Option<*mut RawValue> {
    if receiver.0 & TAG_MASK != OBJECT_TAG {
        return None;
    }
    let object = (receiver.0 & PAYLOAD_MASK) as usize as *const usize;
    let cache = unsafe { (*set_site).literal as usize as *const RawPropertyIc };
    let expected_shape = unsafe { (*cache).receiver_shape };
    if expected_shape.is_null() {
        return None;
    }
    let actual_shape = unsafe { *object.add(OBJECT_SHAPE_WORD_OFFSET) } as *const u8;
    if actual_shape != expected_shape {
        return None;
    }
    let slots = unsafe { *object.add(OBJECT_SLOTS_WORD_OFFSET) } as *mut RawValue;
    Some(unsafe { slots.add((*cache).slot) })
}

#[inline(always)]
unsafe fn dense_array_view(receiver: RawValue) -> Option<RawDenseView> {
    if receiver.0 & TAG_MASK != OBJECT_TAG {
        return None;
    }
    let object = (receiver.0 & PAYLOAD_MASK) as usize as *const usize;
    let elements = unsafe { *object.add(OBJECT_DENSE_ELEMENTS_WORD_OFFSET) } as *mut RawValue;
    if elements.is_null() {
        return None;
    }
    Some(RawDenseView {
        elements,
        length: unsafe { *object.add(OBJECT_DENSE_LENGTH_WORD_OFFSET) },
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_get_static(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let receiver = unsafe { *frame_ref.register_values.add(item.left) };
    let Some(value) = (unsafe { read_cached_property(receiver, site) }) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let destination = unsafe { frame_ref.register_values.add(item.dst) };
    let old = unsafe { *destination };
    if needs_reference_count(value) || needs_reference_count(old) {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    unsafe { *destination = value };
    unsafe { become __quench_dyn_hole_next(frame, site.add(1)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_set_static(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let receiver = unsafe { *frame_ref.register_values.add(item.left) };
    let source = unsafe { *frame_ref.register_values.add(item.dst) };
    let Some(destination) = (unsafe { cached_own_property_slot(receiver, site) }) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let old = unsafe { *destination };
    if needs_reference_count(source) || needs_reference_count(old) {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    unsafe { *destination = source };
    unsafe { become __quench_dyn_hole_next(frame, site.add(1)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_get_computed_dense(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let receiver = unsafe { *frame_ref.register_values.add(item.left) };
    let key = unsafe { *frame_ref.register_values.add(item.right) };
    let Some(index) = dense_index(key) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let Some(view) = (unsafe { dense_array_view(receiver) }) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    if index >= view.length {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let value = unsafe { *view.elements.add(index) };
    let destination = unsafe { frame_ref.register_values.add(item.dst) };
    let old = unsafe { *destination };
    if needs_reference_count(value) || needs_reference_count(old) {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    unsafe { *destination = value };
    unsafe { become __quench_dyn_hole_next(frame, site.add(1)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_set_computed_dense(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let receiver = unsafe { *frame_ref.register_values.add(item.left) };
    let key = unsafe { *frame_ref.register_values.add(item.right) };
    let source = unsafe { *frame_ref.register_values.add(item.dst) };
    let Some(index) = dense_index(key) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let Some(view) = (unsafe { dense_array_view(receiver) }) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    if index >= view.length {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let destination = unsafe { view.elements.add(index) };
    let old = unsafe { *destination };
    let preserves_number_count = is_number(source) == is_number(old);
    if !preserves_number_count || needs_reference_count(source) || needs_reference_count(old) {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    unsafe { *destination = source };
    unsafe { become __quench_dyn_hole_next(frame, site.add(1)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_take_two_own_properties_return_undefined(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let first_receiver_load = unsafe { &*site.add(TWO_PROPERTY_STORE_FIRST_RECEIVER_LOAD_INDEX) };
    let first_value_load = unsafe { &*site.add(TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_INDEX) };
    let first_set = unsafe { &*site.add(TWO_PROPERTY_STORE_FIRST_SET_INDEX) };
    let second_receiver_load =
        unsafe { &*site.add(TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_INDEX) };
    let second_value_load = unsafe { &*site.add(TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_INDEX) };
    let second_set = unsafe { &*site.add(TWO_PROPERTY_STORE_SECOND_SET_INDEX) };

    let locals_are_valid = first_receiver_load.left < frame_ref.local_count
        && first_value_load.left < frame_ref.local_count
        && second_receiver_load.left < frame_ref.local_count
        && second_value_load.left < frame_ref.local_count
        && first_value_load.left != second_value_load.left;
    if !locals_are_valid || frame_ref.result.0 != UNDEFINED_TAG {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }

    let first_receiver = unsafe { *frame_ref.local_values.add(first_receiver_load.left) };
    let second_receiver = unsafe { *frame_ref.local_values.add(second_receiver_load.left) };
    let Some(first_property) =
        (unsafe { cached_own_property_slot(first_receiver, first_set) })
    else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let Some(second_property) =
        (unsafe { cached_own_property_slot(second_receiver, second_set) })
    else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };

    let first_source = unsafe { frame_ref.local_values.add(first_value_load.left) };
    let second_source = unsafe { frame_ref.local_values.add(second_value_load.left) };
    unsafe {
        core::ptr::swap(first_source, first_property);
        core::ptr::swap(second_source, second_property);
        become __quench_dyn_hole_next(frame, site.add(TWO_PROPERTY_STORE_INSTRUCTION_COUNT))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_dead_own_property_store_local_jump(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let load = unsafe { &*site };
    let store = unsafe { &*site.add(PROPERTY_LOAD_STORE_INDEX) };
    if load.left >= frame_ref.local_count || store.dst >= frame_ref.local_count {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let receiver = unsafe { *frame_ref.local_values.add(load.left) };
    let property = unsafe {
        read_cached_property(receiver, site.add(PROPERTY_LOAD_GET_INDEX))
    };
    let Some(property) = property else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let destination = unsafe { frame_ref.local_values.add(store.dst) };
    if needs_reference_count(property) || needs_reference_count(unsafe { *destination }) {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    unsafe { *destination = property };
    let jump = unsafe { &*site.add(PROPERTY_LOAD_JUMP_INDEX) };
    let target_site = unsafe { frame_ref.sites.add(jump.literal as usize) };
    unsafe { become __quench_dyn_hole_next(frame, target_site) }
}

#[inline(always)]
fn strict_equal_without_string_content(left: RawValue, right: RawValue) -> u8 {
    if is_number(left) {
        return if is_number(right) && f64::from_bits(left.0) == f64::from_bits(right.0) {
            TRUTHINESS_TRUE
        } else {
            TRUTHINESS_FALSE
        };
    }
    if is_number(right) {
        return TRUTHINESS_FALSE;
    }
    let left_tag = left.0 & TAG_MASK;
    let right_tag = right.0 & TAG_MASK;
    if left_tag == STRING_TAG || right_tag == STRING_TAG {
        return if left_tag == right_tag {
            TRUTHINESS_NEEDS_SLOW_PATH
        } else {
            TRUTHINESS_FALSE
        };
    }
    if left.0 == right.0 {
        TRUTHINESS_TRUE
    } else {
        TRUTHINESS_FALSE
    }
}

#[inline(always)]
fn loose_equal_without_string_conversion(left: RawValue, right: RawValue) -> u8 {
    let strict = strict_equal_without_string_content(left, right);
    if strict != TRUTHINESS_FALSE {
        return strict;
    }

    let left_tag = left.0 & TAG_MASK;
    let right_tag = right.0 & TAG_MASK;
    let left_is_nullish = left_tag == NULL_TAG || left_tag == UNDEFINED_TAG;
    let right_is_nullish = right_tag == NULL_TAG || right_tag == UNDEFINED_TAG;
    if left_is_nullish || right_is_nullish {
        return if left_is_nullish && right_is_nullish {
            TRUTHINESS_TRUE
        } else if left_tag == BOOL_TAG {
            let left_number = (left.0 & PAYLOAD_MASK) as f64;
            let right_number = if right_tag == NULL_TAG { 0.0 } else { f64::NAN };
            if left_number == right_number {
                TRUTHINESS_TRUE
            } else {
                TRUTHINESS_FALSE
            }
        } else if right_tag == BOOL_TAG {
            let left_number = if left_tag == NULL_TAG { 0.0 } else { f64::NAN };
            let right_number = (right.0 & PAYLOAD_MASK) as f64;
            if left_number == right_number {
                TRUTHINESS_TRUE
            } else {
                TRUTHINESS_FALSE
            }
        } else {
            TRUTHINESS_FALSE
        };
    }

    if left_tag == BOOL_TAG {
        if is_number(right) {
            return if (left.0 & PAYLOAD_MASK) as f64 == f64::from_bits(right.0) {
                TRUTHINESS_TRUE
            } else {
                TRUTHINESS_FALSE
            };
        }
        return if right_tag == STRING_TAG {
            TRUTHINESS_NEEDS_SLOW_PATH
        } else {
            TRUTHINESS_FALSE
        };
    }
    if right_tag == BOOL_TAG {
        if is_number(left) {
            return if f64::from_bits(left.0) == (right.0 & PAYLOAD_MASK) as f64 {
                TRUTHINESS_TRUE
            } else {
                TRUTHINESS_FALSE
            };
        }
        return if left_tag == STRING_TAG {
            TRUTHINESS_NEEDS_SLOW_PATH
        } else {
            TRUTHINESS_FALSE
        };
    }
    if (is_number(left) && right_tag == STRING_TAG)
        || (left_tag == STRING_TAG && is_number(right))
    {
        return TRUTHINESS_NEEDS_SLOW_PATH;
    }
    TRUTHINESS_FALSE
}

macro_rules! define_dyn_equality {
    ($name:ident, $compare:ident, $invert:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let item = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(item.left) };
            let right = unsafe { *frame_ref.register_values.add(item.right) };
            let destination = unsafe { frame_ref.register_values.add(item.dst) };
            let old = unsafe { *destination };
            let outcome = $compare(left, right);
            if outcome == TRUTHINESS_NEEDS_SLOW_PATH || needs_reference_count(old) {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let mut result = outcome == TRUTHINESS_TRUE;
            if $invert {
                result = !result;
            }
            let payload = if result { TRUE_PAYLOAD } else { FALSE_PAYLOAD };
            unsafe { *destination = RawValue(BOOL_TAG | payload) };
            unsafe { become __quench_dyn_hole_next(frame, site.add(1)) }
        }
    };
}

define_dyn_equality!(
    quench_dyn_strict_equal,
    strict_equal_without_string_content,
    false
);
define_dyn_equality!(
    quench_dyn_strict_not_equal,
    strict_equal_without_string_content,
    true
);
define_dyn_equality!(
    quench_dyn_equal,
    loose_equal_without_string_conversion,
    false
);
define_dyn_equality!(
    quench_dyn_not_equal,
    loose_equal_without_string_conversion,
    true
);

#[inline(always)]
fn dense_index(value: RawValue) -> Option<usize> {
    let number = f64::from_bits(value.0);
    if !number.is_finite() || number < 0.0 || number > MAX_JS_ARRAY_INDEX {
        return None;
    }
    let index = number as usize;
    ((index as f64) == number).then_some(index)
}

#[inline(always)]
fn proven_dense_index(value: RawValue) -> Option<usize> {
    let number = f64::from_bits(value.0);
    (number <= MAX_JS_ARRAY_INDEX).then_some(number as usize)
}

#[inline(always)]
fn js_u32(value: f64) -> u32 {
    let bits = value.to_bits();
    let raw_exponent = (bits >> F64_EXPONENT_SHIFT) & F64_EXPONENT_MASK;
    let exponent = raw_exponent as i32 - F64_EXPONENT_BIAS;
    let significand = F64_IMPLICIT_BIT | (bits & F64_FRACTION_MASK);
    let left_shift = (exponent - F64_EXPONENT_SHIFT as i32).clamp(0, UINT32_BITS - 1) as u32;
    let right_shift =
        (F64_EXPONENT_SHIFT as i32 - exponent).clamp(0, MAX_U64_SHIFT) as u32;
    let magnitude = if exponent >= F64_EXPONENT_SHIFT as i32 {
        (significand << left_shift) as u32
    } else {
        (significand >> right_shift) as u32
    };
    let valid = raw_exponent != F64_EXPONENT_MASK
        && raw_exponent != 0
        && exponent >= 0
        && exponent < F64_EXPONENT_SHIFT as i32 + UINT32_BITS;
    let valid_mask = 0_u32.wrapping_sub(u32::from(valid));
    let magnitude = magnitude & valid_mask;
    let sign_mask = 0_u32.wrapping_sub(u32::from(value.is_sign_negative()));
    (magnitude ^ sign_mask).wrapping_sub(sign_mask)
}

#[inline(always)]
fn js_i32(value: f64) -> i32 {
    js_u32(value) as i32
}

include!("register_region.rs");

#[inline(always)]
fn immediate_truthiness(value: RawValue) -> u8 {
    if is_number(value) {
        let number = f64::from_bits(value.0);
        return if number != 0.0 && !number.is_nan() {
            TRUTHINESS_TRUE
        } else {
            TRUTHINESS_FALSE
        };
    }
    let tag = value.0 & TAG_MASK;
    if tag < STRING_TAG {
        if tag == BOOL_TAG && value.0 & !TAG_MASK == TRUE_PAYLOAD {
            TRUTHINESS_TRUE
        } else {
            TRUTHINESS_FALSE
        }
    } else if tag == STRING_TAG {
        TRUTHINESS_NEEDS_SLOW_PATH
    } else {
        TRUTHINESS_TRUE
    }
}

macro_rules! continue_or_slow {
    ($frame:ident, $site:ident, $guard:expr, $effect:block) => {
        if $guard {
            $effect
            unsafe { become __quench_dyn_hole_next($frame, $site.add(1)) }
        } else {
            unsafe { become __quench_dyn_hole_slow($frame, $site) }
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_load_literal(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let raw_frame = frame;
    let frame = unsafe { &mut *raw_frame };
    let site_ref = unsafe { &*site };
    let destination = unsafe { frame.register_values.add(site_ref.dst) };
    let old = unsafe { *destination };
    continue_or_slow!(raw_frame, site, !needs_reference_count(old), {
        unsafe { *destination = RawValue(site_ref.literal) };
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_load_local(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let raw_frame = frame;
    let frame_ref = unsafe { &mut *raw_frame };
    let site_ref = unsafe { &*site };
    let in_bounds = site_ref.left < frame_ref.local_count;
    let source = if in_bounds {
        unsafe { *frame_ref.local_values.add(site_ref.left) }
    } else {
        RawValue(FIRST_HEAP_TAG)
    };
    let destination = unsafe { frame_ref.register_values.add(site_ref.dst) };
    let old = unsafe { *destination };
    continue_or_slow!(raw_frame, site, in_bounds && !needs_reference_count(source) && !needs_reference_count(old), {
        unsafe { *destination = source };
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_load_name_cached(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let cache = unsafe { *frame_ref.name_ics.add(item.pc) };
    if cache.layout.is_null() || cache.depth >= frame_ref.environment_access_chain_len {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let access = unsafe { *frame_ref.environment_access_chain.add(cache.depth) };
    if access.is_null() {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let access = unsafe { &*access };
    if access.layout != cache.layout || cache.slot >= access.len {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let source = unsafe { *access.values.add(cache.slot) };
    let destination = unsafe { frame_ref.register_values.add(item.dst) };
    let old = unsafe { *destination };
    continue_or_slow!(
        frame,
        site,
        !needs_reference_count(source) && !needs_reference_count(old),
        {
            unsafe { *destination = source };
        }
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_store_local(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let raw_frame = frame;
    let frame_ref = unsafe { &mut *raw_frame };
    let site_ref = unsafe { &*site };
    let in_bounds = site_ref.dst < frame_ref.local_count;
    let source = unsafe { *frame_ref.register_values.add(site_ref.left) };
    let destination = if in_bounds {
        unsafe { frame_ref.local_values.add(site_ref.dst) }
    } else {
        frame_ref.local_values
    };
    let old = if in_bounds {
        unsafe { *destination }
    } else {
        RawValue(FIRST_HEAP_TAG)
    };
    continue_or_slow!(raw_frame, site, in_bounds && !needs_reference_count(source) && !needs_reference_count(old), {
        unsafe { *destination = source };
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_move(frame: *mut RawDynFrame, site: *const InlineSite) {
    let raw_frame = frame;
    let frame_ref = unsafe { &mut *raw_frame };
    let site_ref = unsafe { &*site };
    let source = unsafe { *frame_ref.register_values.add(site_ref.left) };
    let destination = unsafe { frame_ref.register_values.add(site_ref.dst) };
    let old = unsafe { *destination };
    continue_or_slow!(raw_frame, site, !needs_reference_count(source) && !needs_reference_count(old), {
        unsafe { *destination = source };
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_not(frame: *mut RawDynFrame, site: *const InlineSite) {
    let raw_frame = frame;
    let frame_ref = unsafe { &mut *raw_frame };
    let item = unsafe { &*site };
    let source = unsafe { *frame_ref.register_values.add(item.left) };
    let destination = unsafe { frame_ref.register_values.add(item.dst) };
    let old = unsafe { *destination };
    let truthiness = immediate_truthiness(source);
    continue_or_slow!(
        raw_frame,
        site,
        truthiness != TRUTHINESS_NEEDS_SLOW_PATH && !needs_reference_count(old),
        {
            let payload = if truthiness == TRUTHINESS_FALSE {
                TRUE_PAYLOAD
            } else {
                FALSE_PAYLOAD
            };
            unsafe { *destination = RawValue(BOOL_TAG | payload) };
        }
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_dead_own_property_strict_equal(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &*frame };
    let first_load = unsafe { &*site };
    let second_load = unsafe { &*site.add(PROPERTY_EQUAL_SECOND_LOAD_INDEX) };
    let outcome = if first_load.left < frame_ref.local_count
        && second_load.left < frame_ref.local_count
    {
        let first_receiver = unsafe { *frame_ref.local_values.add(first_load.left) };
        let second_receiver = unsafe { *frame_ref.local_values.add(second_load.left) };
        let first = unsafe {
            read_cached_property(first_receiver, site.add(PROPERTY_EQUAL_FIRST_GET_INDEX))
        };
        let second = unsafe {
            read_cached_property(second_receiver, site.add(PROPERTY_EQUAL_SECOND_GET_INDEX))
        };
        match (first, second) {
            (Some(first), Some(second)) => strict_equal_without_string_content(first, second),
            _ => TRUTHINESS_NEEDS_SLOW_PATH,
        }
    } else {
        TRUTHINESS_NEEDS_SLOW_PATH
    };
    match outcome {
        TRUTHINESS_TRUE => unsafe {
            become __quench_dyn_hole_next(frame, site.add(PROPERTY_EQUAL_INSTRUCTION_COUNT))
        },
        TRUTHINESS_FALSE => {
            let branch = unsafe { &*site.add(PROPERTY_EQUAL_BRANCH_INDEX) };
            let target_site = unsafe { frame_ref.sites.add(branch.literal as usize) };
            unsafe { become __quench_dyn_hole_branch(frame, target_site) }
        }
        _ => unsafe { become __quench_dyn_hole_slow(frame, site) },
    }
}

macro_rules! region_continue {
    ($frame:ident, $site:ident) => {
        unsafe {
            let next_site = $site
                .cast::<u8>()
                .add(site_holes::NEXT_SITE_BYTE_OFFSET_HOLE)
                .cast::<InlineSite>();
            become __quench_dyn_hole_next($frame, next_site)
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_nop(frame: *mut RawDynFrame, site: *const InlineSite) {
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_guard(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let pc = unsafe { (*site).pc };
    let guard = unsafe { (*frame).region_guard };
    let fallback = unsafe { guard(frame, pc) };
    if fallback != 0 {
        let continuation: unsafe extern "C" fn(*mut RawDynFrame, *const InlineSite) =
            unsafe { core::mem::transmute(fallback) };
        let next_site = unsafe { (*frame).current_site };
        unsafe { become continuation(frame, next_site) }
    }
    unsafe { become __quench_dyn_hole_next(frame, site) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_counted_jump(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    frame_ref.region_iterations = frame_ref.region_iterations.saturating_add(1);
    let target_pc = unsafe { (*site).literal as usize };
    let target_site = unsafe { frame_ref.sites.add(target_pc) };
    unsafe { become __quench_dyn_hole_next(frame, target_site) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_load_literal(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    unsafe { *frame_ref.register_values.add(item.dst) = RawValue(item.literal) };
    region_continue!(frame, site);
}

macro_rules! define_burned_literal_stencil {
    ($name:ident, $hole_bits:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::DESTINATION_REGISTER_HOLE_SLOT) = RawValue($hole_bits)
            };
            region_continue!(frame, site);
        }
    };
}

define_burned_literal_stencil!(
    quench_region_burned_load_literal_one_lane,
    raw_value_holes::ONE_HIGH_LANE_HOLE_BITS
);
define_burned_literal_stencil!(
    quench_region_burned_load_literal_two_lanes,
    raw_value_holes::TWO_HIGH_LANES_HOLE_BITS
);
define_burned_literal_stencil!(
    quench_region_burned_load_literal_three_lanes,
    raw_value_holes::THREE_HIGH_LANES_HOLE_BITS
);
define_burned_literal_stencil!(
    quench_region_burned_load_literal_four_lanes,
    raw_value_holes::RAW_VALUE_HOLE_BITS
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_load_local(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let value = unsafe { *frame_ref.local_values.add(item.left) };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_burned_load_local(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let value = unsafe {
        *frame_ref
            .local_values
            .add(operand_holes::SOURCE_LOCAL_HOLE_SLOT)
    };
    unsafe {
        *frame_ref
            .register_values
            .add(operand_holes::DESTINATION_REGISTER_HOLE_SLOT) = value
    };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_load_name(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let raw_frame = frame;
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let snapshot_slot = item.literal as usize;
    let value = unsafe { *frame_ref.name_snapshots.add(snapshot_slot) };
    let destination = unsafe { frame_ref.register_values.add(item.dst) };
    let old = unsafe { *destination };
    continue_or_slow!(
        raw_frame,
        site,
        !needs_reference_count(value) && !needs_reference_count(old),
        {
            unsafe { *destination = value };
        }
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_store_local(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let value = unsafe { *frame_ref.register_values.add(item.left) };
    unsafe { *frame_ref.local_values.add(item.dst) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_burned_store_local(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let value = unsafe {
        *frame_ref
            .register_values
            .add(operand_holes::SOURCE_REGISTER_HOLE_SLOT)
    };
    unsafe {
        *frame_ref
            .local_values
            .add(operand_holes::DESTINATION_LOCAL_HOLE_SLOT) = value
    };
    region_continue!(frame, site);
}

macro_rules! define_region_dead_local_update {
    ($name:ident, $update_operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(UPDATE_LOAD_LOCAL_INDEX) };
            let literal = unsafe { &*site.add(UPDATE_LITERAL_INDEX) };
            let store = unsafe { &*site.add(UPDATE_STORE_LOCAL_INDEX) };
            let local = unsafe { *frame_ref.local_values.add(load.left) };
            let result = canonical_number(
                f64::from_bits(local.0)
                    $update_operator f64::from_bits(literal.literal),
            );
            unsafe { *frame_ref.local_values.add(store.dst) = result };
            unsafe {
                become __quench_dyn_hole_next(
                    frame,
                    site.add(LOCAL_UPDATE_INSTRUCTION_COUNT),
                )
            }
        }
    };
}

define_region_dead_local_update!(quench_region_dead_local_number_add, +);
define_region_dead_local_update!(quench_region_dead_local_number_subtract, -);

macro_rules! define_region_dead_update_jump {
    ($name:ident, $update_operator:tt, $count_iteration:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(UPDATE_LOAD_LOCAL_INDEX) };
            let literal = unsafe { &*site.add(UPDATE_LITERAL_INDEX) };
            let store = unsafe { &*site.add(UPDATE_STORE_LOCAL_INDEX) };
            let jump = unsafe { &*site.add(UPDATE_JUMP_INDEX) };
            let local = unsafe { *frame_ref.local_values.add(load.left) };
            let result = canonical_number(
                f64::from_bits(local.0)
                    $update_operator f64::from_bits(literal.literal),
            );
            unsafe { *frame_ref.local_values.add(store.dst) = result };
            if $count_iteration {
                frame_ref.region_iterations = frame_ref.region_iterations.saturating_add(1);
            }
            let target_site = unsafe { frame_ref.sites.add(jump.literal as usize) };
            unsafe { become __quench_dyn_hole_next(frame, target_site) }
        }
    };
}

define_region_dead_update_jump!(
    quench_region_dead_update_local_number_add_jump,
    +,
    false
);
define_region_dead_update_jump!(
    quench_region_dead_update_local_number_subtract_jump,
    -,
    false
);
define_region_dead_update_jump!(
    quench_region_counted_dead_update_local_number_add_jump,
    +,
    true
);
define_region_dead_update_jump!(
    quench_region_counted_dead_update_local_number_subtract_jump,
    -,
    true
);

macro_rules! finish_region_condition {
    ($frame:ident, $frame_ref:ident, $site:ident, $result:ident) => {
        if $result {
            unsafe {
                become __quench_dyn_hole_next(
                    $frame,
                    $site.add(CONDITION_INSTRUCTION_COUNT),
                )
            }
        } else {
            let branch = unsafe { &*$site.add(CONDITION_BRANCH_INDEX) };
            let target_site = unsafe { $frame_ref.sites.add(branch.literal as usize) };
            unsafe { become __quench_dyn_hole_branch($frame, target_site) }
        }
    };
}

macro_rules! define_region_local_local_condition {
    ($name:ident, $compare_operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &*frame };
            let left_site = unsafe { &*site.add(CONDITION_LOAD_LOCAL_INDEX) };
            let right_site = unsafe { &*site.add(CONDITION_RIGHT_OPERAND_SITE_INDEX) };
            let left = unsafe { *frame_ref.local_values.add(left_site.left) };
            let right = unsafe { *frame_ref.local_values.add(right_site.left) };
            let result = f64::from_bits(left.0) $compare_operator f64::from_bits(right.0);
            finish_region_condition!(frame, frame_ref, site, result);
        }
    };
}

macro_rules! define_region_local_literal_condition {
    ($name:ident, $compare_operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &*frame };
            let left_site = unsafe { &*site.add(CONDITION_LOAD_LOCAL_INDEX) };
            let right_site = unsafe { &*site.add(CONDITION_RIGHT_OPERAND_SITE_INDEX) };
            let left = unsafe { *frame_ref.local_values.add(left_site.left) };
            let result = f64::from_bits(left.0)
                $compare_operator f64::from_bits(right_site.literal);
            finish_region_condition!(frame, frame_ref, site, result);
        }
    };
}

macro_rules! define_region_local_name_condition {
    ($name:ident, $compare_operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &*frame };
            let left_site = unsafe { &*site.add(CONDITION_LOAD_LOCAL_INDEX) };
            let right_site = unsafe { &*site.add(NAME_CONDITION_LOAD_NAME_INDEX) };
            let left = unsafe { *frame_ref.local_values.add(left_site.left) };
            let snapshot_slot = right_site.literal as usize;
            let right = unsafe { *frame_ref.name_snapshots.add(snapshot_slot) };
            let result = f64::from_bits(left.0) $compare_operator f64::from_bits(right.0);
            finish_region_condition!(frame, frame_ref, site, result);
        }
    };
}

macro_rules! define_region_condition_family {
    (
        $definition_macro:ident,
        $equal:ident,
        $not_equal:ident,
        $less:ident,
        $less_equal:ident,
        $greater:ident,
        $greater_equal:ident
    ) => {
        $definition_macro!($equal, ==);
        $definition_macro!($not_equal, !=);
        $definition_macro!($less, <);
        $definition_macro!($less_equal, <=);
        $definition_macro!($greater, >);
        $definition_macro!($greater_equal, >=);
    };
}

define_region_condition_family!(
    define_region_local_local_condition,
    quench_region_dead_condition_local_local_number_equal,
    quench_region_dead_condition_local_local_number_not_equal,
    quench_region_dead_condition_local_local_number_less,
    quench_region_dead_condition_local_local_number_less_equal,
    quench_region_dead_condition_local_local_number_greater,
    quench_region_dead_condition_local_local_number_greater_equal
);
define_region_condition_family!(
    define_region_local_literal_condition,
    quench_region_dead_condition_local_literal_number_equal,
    quench_region_dead_condition_local_literal_number_not_equal,
    quench_region_dead_condition_local_literal_number_less,
    quench_region_dead_condition_local_literal_number_less_equal,
    quench_region_dead_condition_local_literal_number_greater,
    quench_region_dead_condition_local_literal_number_greater_equal
);
define_region_condition_family!(
    define_region_local_name_condition,
    quench_region_dead_condition_local_name_number_equal,
    quench_region_dead_condition_local_name_number_not_equal,
    quench_region_dead_condition_local_name_number_less,
    quench_region_dead_condition_local_name_number_less_equal,
    quench_region_dead_condition_local_name_number_greater,
    quench_region_dead_condition_local_name_number_greater_equal
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_move(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let value = unsafe { *frame_ref.register_values.add(item.left) };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_burned_move(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let value = unsafe {
        *frame_ref
            .register_values
            .add(operand_holes::SOURCE_REGISTER_HOLE_SLOT)
    };
    unsafe {
        *frame_ref
            .register_values
            .add(operand_holes::DESTINATION_REGISTER_HOLE_SLOT) = value
    };
    region_continue!(frame, site);
}

macro_rules! define_region_number_binary {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let item = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(item.left) };
            let right = unsafe { *frame_ref.register_values.add(item.right) };
            let result = f64::from_bits(left.0) $operator f64::from_bits(right.0);
            unsafe { *frame_ref.register_values.add(item.dst) = canonical_number(result) };
            region_continue!(frame, site);
        }
    };
}

define_region_number_binary!(quench_region_add, +);
define_region_number_binary!(quench_region_subtract, -);
define_region_number_binary!(quench_region_multiply, *);
define_region_number_binary!(quench_region_divide, /);

macro_rules! define_region_burned_number_binary {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let left = unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::LEFT_REGISTER_HOLE_SLOT)
            };
            let right = unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::RIGHT_REGISTER_HOLE_SLOT)
            };
            let result = f64::from_bits(left.0) $operator f64::from_bits(right.0);
            unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::DESTINATION_REGISTER_HOLE_SLOT) = canonical_number(result)
            };
            region_continue!(frame, site);
        }
    };
}

define_region_burned_number_binary!(quench_region_burned_add, +);
define_region_burned_number_binary!(quench_region_burned_subtract, -);
define_region_burned_number_binary!(quench_region_burned_multiply, *);
define_region_burned_number_binary!(quench_region_burned_divide, /);

macro_rules! define_region_number_compare {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let item = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(item.left) };
            let right = unsafe { *frame_ref.register_values.add(item.right) };
            let result = f64::from_bits(left.0) $operator f64::from_bits(right.0);
            let payload = if result { TRUE_PAYLOAD } else { FALSE_PAYLOAD };
            unsafe { *frame_ref.register_values.add(item.dst) = RawValue(BOOL_TAG | payload) };
            region_continue!(frame, site);
        }
    };
}

define_region_number_compare!(quench_region_equal, ==);
define_region_number_compare!(quench_region_not_equal, !=);
define_region_number_compare!(quench_region_less, <);
define_region_number_compare!(quench_region_less_equal, <=);
define_region_number_compare!(quench_region_greater, >);
define_region_number_compare!(quench_region_greater_equal, >=);

macro_rules! define_region_burned_number_compare {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let left = unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::LEFT_REGISTER_HOLE_SLOT)
            };
            let right = unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::RIGHT_REGISTER_HOLE_SLOT)
            };
            let result = f64::from_bits(left.0) $operator f64::from_bits(right.0);
            let payload = if result { TRUE_PAYLOAD } else { FALSE_PAYLOAD };
            unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::DESTINATION_REGISTER_HOLE_SLOT) =
                    RawValue(BOOL_TAG | payload)
            };
            region_continue!(frame, site);
        }
    };
}

define_region_burned_number_compare!(quench_region_burned_equal, ==);
define_region_burned_number_compare!(quench_region_burned_not_equal, !=);
define_region_burned_number_compare!(quench_region_burned_less, <);
define_region_burned_number_compare!(quench_region_burned_less_equal, <=);
define_region_burned_number_compare!(quench_region_burned_greater, >);
define_region_burned_number_compare!(quench_region_burned_greater_equal, >=);

macro_rules! define_region_bitwise_binary {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let item = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(item.left) };
            let right = unsafe { *frame_ref.register_values.add(item.right) };
            let result = js_i32(f64::from_bits(left.0)) $operator js_i32(f64::from_bits(right.0));
            unsafe {
                *frame_ref.register_values.add(item.dst) = RawValue((f64::from(result)).to_bits())
            };
            region_continue!(frame, site);
        }
    };
}

define_region_bitwise_binary!(quench_region_bit_or, |);
define_region_bitwise_binary!(quench_region_bit_xor, ^);
define_region_bitwise_binary!(quench_region_bit_and, &);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_shift_left(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let left = unsafe { *frame_ref.register_values.add(item.left) };
    let right = unsafe { *frame_ref.register_values.add(item.right) };
    let count = js_u32(f64::from_bits(right.0)) & SHIFT_COUNT_MASK;
    let result = js_i32(f64::from_bits(left.0)).wrapping_shl(count);
    unsafe { *frame_ref.register_values.add(item.dst) = RawValue((f64::from(result)).to_bits()) };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_shift_right(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let left = unsafe { *frame_ref.register_values.add(item.left) };
    let right = unsafe { *frame_ref.register_values.add(item.right) };
    let count = js_u32(f64::from_bits(right.0)) & SHIFT_COUNT_MASK;
    let result = js_i32(f64::from_bits(left.0)).wrapping_shr(count);
    unsafe { *frame_ref.register_values.add(item.dst) = RawValue((f64::from(result)).to_bits()) };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_shift_right_unsigned(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let left = unsafe { *frame_ref.register_values.add(item.left) };
    let right = unsafe { *frame_ref.register_values.add(item.right) };
    let count = js_u32(f64::from_bits(right.0)) & SHIFT_COUNT_MASK;
    let result = js_u32(f64::from_bits(left.0)).wrapping_shr(count);
    unsafe { *frame_ref.register_values.add(item.dst) = RawValue((f64::from(result)).to_bits()) };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_unary_plus(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let source = unsafe { *frame_ref.register_values.add(item.left) };
    let result = canonical_number(f64::from_bits(source.0));
    unsafe { *frame_ref.register_values.add(item.dst) = result };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_negate(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let source = unsafe { *frame_ref.register_values.add(item.left) };
    let result = canonical_number(-f64::from_bits(source.0));
    unsafe { *frame_ref.register_values.add(item.dst) = result };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_bit_not(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let source = unsafe { *frame_ref.register_values.add(item.left) };
    let result = !js_i32(f64::from_bits(source.0));
    unsafe { *frame_ref.register_values.add(item.dst) = RawValue((f64::from(result)).to_bits()) };
    region_continue!(frame, site);
}

#[inline(always)]
fn burned_unary_negate(source: f64) -> RawValue {
    canonical_number(-source)
}

#[inline(always)]
fn burned_unary_bit_not(source: f64) -> RawValue {
    RawValue((f64::from(!js_i32(source))).to_bits())
}

macro_rules! define_region_burned_unary {
    ($name:ident, $operation:path) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let source = unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::SOURCE_REGISTER_HOLE_SLOT)
            };
            let result = $operation(f64::from_bits(source.0));
            unsafe {
                *frame_ref
                    .register_values
                    .add(operand_holes::DESTINATION_REGISTER_HOLE_SLOT) = result
            };
            region_continue!(frame, site);
        }
    };
}

define_region_burned_unary!(quench_region_burned_unary_plus, canonical_number);
define_region_burned_unary!(quench_region_burned_negate, burned_unary_negate);
define_region_burned_unary!(quench_region_burned_bit_not, burned_unary_bit_not);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_read_dense(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let index = unsafe { *frame_ref.register_values.add(item.right) };
    let Some(index) = dense_index(index) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    if index >= view.length {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let value = unsafe { *view.elements.add(index) };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_read_dense_proven_index(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let index = unsafe { *frame_ref.register_values.add(item.right) };
    let Some(index) = proven_dense_index(index) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    if index >= view.length {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let value = unsafe { *view.elements.add(index) };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_write_dense(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let index = unsafe { *frame_ref.register_values.add(item.right) };
    let Some(index) = dense_index(index) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    if index >= view.length {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let value = unsafe { *frame_ref.register_values.add(item.dst) };
    unsafe { *view.elements.add(index) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_write_dense_proven_index(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let index = unsafe { *frame_ref.register_values.add(item.right) };
    let Some(index) = proven_dense_index(index) else {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    if index >= view.length {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let value = unsafe { *frame_ref.register_values.add(item.dst) };
    unsafe { *view.elements.add(index) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_read_static(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    let value = unsafe { *view.elements };
    unsafe { *frame_ref.register_values.add(item.dst) = value };
    region_continue!(frame, site);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_region_write_static(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let item = unsafe { &*site };
    let view = unsafe { *frame_ref.region_arrays.add(item.literal as usize) };
    let value = unsafe { *frame_ref.register_values.add(item.dst) };
    unsafe { *view.elements = value };
    region_continue!(frame, site);
}

macro_rules! define_dyn_number_binary {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let raw_frame = frame;
            let frame_ref = unsafe { &mut *raw_frame };
            let site_ref = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(site_ref.left) };
            let right = unsafe { *frame_ref.register_values.add(site_ref.right) };
            let destination = unsafe { frame_ref.register_values.add(site_ref.dst) };
            let old = unsafe { *destination };
            continue_or_slow!(raw_frame, site, is_number(left) && is_number(right) && !needs_reference_count(old), {
                let value = f64::from_bits(left.0) $operator f64::from_bits(right.0);
                unsafe { *destination = canonical_number(value) };
            });
        }
    };
}

define_dyn_number_binary!(quench_dyn_add, +);
define_dyn_number_binary!(quench_dyn_subtract, -);
define_dyn_number_binary!(quench_dyn_multiply, *);
define_dyn_number_binary!(quench_dyn_divide, /);

macro_rules! define_dyn_bitwise_binary {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let raw_frame = frame;
            let frame_ref = unsafe { &mut *raw_frame };
            let site_ref = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(site_ref.left) };
            let right = unsafe { *frame_ref.register_values.add(site_ref.right) };
            let destination = unsafe { frame_ref.register_values.add(site_ref.dst) };
            let old = unsafe { *destination };
            continue_or_slow!(raw_frame, site, is_number(left) && is_number(right) && !needs_reference_count(old), {
                let result = js_i32(f64::from_bits(left.0)) $operator js_i32(f64::from_bits(right.0));
                unsafe { *destination = RawValue(f64::from(result).to_bits()) };
            });
        }
    };
}

define_dyn_bitwise_binary!(quench_dyn_bit_or, |);
define_dyn_bitwise_binary!(quench_dyn_bit_xor, ^);
define_dyn_bitwise_binary!(quench_dyn_bit_and, &);

macro_rules! define_dyn_shift {
    ($name:ident, $convert:ident, $shift:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let raw_frame = frame;
            let frame_ref = unsafe { &mut *raw_frame };
            let site_ref = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(site_ref.left) };
            let right = unsafe { *frame_ref.register_values.add(site_ref.right) };
            let destination = unsafe { frame_ref.register_values.add(site_ref.dst) };
            let old = unsafe { *destination };
            continue_or_slow!(raw_frame, site, is_number(left) && is_number(right) && !needs_reference_count(old), {
                let count = js_u32(f64::from_bits(right.0)) & SHIFT_COUNT_MASK;
                let result = $convert(f64::from_bits(left.0)).$shift(count);
                unsafe { *destination = RawValue(f64::from(result).to_bits()) };
            });
        }
    };
}

define_dyn_shift!(quench_dyn_shift_left, js_i32, wrapping_shl);
define_dyn_shift!(quench_dyn_shift_right, js_i32, wrapping_shr);
define_dyn_shift!(quench_dyn_shift_right_unsigned, js_u32, wrapping_shr);

macro_rules! define_dyn_number_compare {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let raw_frame = frame;
            let frame_ref = unsafe { &mut *raw_frame };
            let site_ref = unsafe { &*site };
            let left = unsafe { *frame_ref.register_values.add(site_ref.left) };
            let right = unsafe { *frame_ref.register_values.add(site_ref.right) };
            let destination = unsafe { frame_ref.register_values.add(site_ref.dst) };
            let old = unsafe { *destination };
            continue_or_slow!(raw_frame, site, is_number(left) && is_number(right) && !needs_reference_count(old), {
                let value = f64::from_bits(left.0) $operator f64::from_bits(right.0);
                let payload = if value { TRUE_PAYLOAD } else { FALSE_PAYLOAD };
                unsafe { *destination = RawValue(BOOL_TAG | payload) };
            });
        }
    };
}

define_dyn_number_compare!(quench_dyn_less, <);
define_dyn_number_compare!(quench_dyn_less_equal, <=);
define_dyn_number_compare!(quench_dyn_greater, >);
define_dyn_number_compare!(quench_dyn_greater_equal, >=);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_constant_truthy(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    unsafe {
        become __quench_dyn_hole_next(
            frame,
            site.add(CONSTANT_CONDITION_INSTRUCTION_COUNT),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_constant_falsey(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let branch = unsafe { &*site.add(CONSTANT_CONDITION_BRANCH_INDEX) };
    let target_site = unsafe { frame_ref.sites.add(branch.literal as usize) };
    unsafe { become __quench_dyn_hole_next(frame, target_site) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_return(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let return_site = unsafe { &*site };
    if needs_reference_count(frame_ref.result) {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let source = unsafe { frame_ref.register_values.add(return_site.left) };
    frame_ref.result = unsafe { *source };
    unsafe { *source = RawValue(UNDEFINED_TAG) };
    unsafe { become __quench_dyn_hole_next(frame, site.add(1)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_return_undefined(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    if needs_reference_count(frame_ref.result) {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    frame_ref.result = RawValue(UNDEFINED_TAG);
    unsafe { become __quench_dyn_hole_next(frame, site.add(1)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_return_local(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let load = unsafe { &*site };
    let return_site = unsafe { &*site.add(1) };
    let local_in_bounds = load.left < frame_ref.local_count;
    let value = if local_in_bounds {
        unsafe { *frame_ref.local_values.add(load.left) }
    } else {
        RawValue(FIRST_HEAP_TAG)
    };
    let guard = local_in_bounds
        && return_site.left == load.dst
        && !needs_reference_count(value)
        && !needs_reference_count(frame_ref.result);
    continue_or_slow!(frame, site, guard, {
        frame_ref.result = value;
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_return_literal(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let literal = unsafe { &*site };
    let return_site = unsafe { &*site.add(1) };
    let value = RawValue(literal.literal);
    let guard = return_site.left == literal.dst
        && !needs_reference_count(value)
        && !needs_reference_count(frame_ref.result);
    continue_or_slow!(frame, site, guard, {
        frame_ref.result = value;
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_loop_add_local_literal(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &mut *frame };
    let load = unsafe { &*site.add(LOOP_ADD_LOAD_LOCAL_INDEX) };
    let literal = unsafe { &*site.add(LOOP_ADD_LOAD_LITERAL_INDEX) };
    let binary = unsafe { &*site.add(LOOP_ADD_BINARY_INDEX) };
    let store = unsafe { &*site.add(LOOP_ADD_STORE_LOCAL_INDEX) };
    let jump = unsafe { &*site.add(LOOP_ADD_JUMP_INDEX) };
    let local_in_bounds = load.left < frame_ref.local_count;
    let store_in_bounds = store.dst < frame_ref.local_count;
    let local = if local_in_bounds {
        unsafe { *frame_ref.local_values.add(load.left) }
    } else {
        RawValue(FIRST_HEAP_TAG)
    };
    let literal_value = RawValue(literal.literal);
    let load_destination = unsafe { frame_ref.register_values.add(load.dst) };
    let literal_destination = unsafe { frame_ref.register_values.add(literal.dst) };
    let binary_destination = unsafe { frame_ref.register_values.add(binary.dst) };
    let store_destination = if store_in_bounds {
        unsafe { frame_ref.local_values.add(store.dst) }
    } else {
        frame_ref.local_values
    };
    let destinations_require_no_rc = !needs_reference_count(unsafe { *load_destination })
        && !needs_reference_count(unsafe { *literal_destination })
        && !needs_reference_count(unsafe { *binary_destination })
        && store_in_bounds
        && !needs_reference_count(unsafe { *store_destination });
    let guard = local_in_bounds
        && store_in_bounds
        && is_number(local)
        && is_number(literal_value)
        && destinations_require_no_rc;
    if !guard {
        unsafe { become __quench_dyn_hole_slow(frame, site) }
    }
    let result = canonical_number(f64::from_bits(local.0) + f64::from_bits(literal_value.0));
    unsafe {
        *load_destination = local;
        *literal_destination = literal_value;
        *binary_destination = result;
        *store_destination = result;
    }
    let target_site = unsafe { frame_ref.sites.add(jump.literal as usize) };
    unsafe { become __quench_dyn_hole_next(frame, target_site) }
}

macro_rules! define_dyn_local_literal_condition {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(CONDITION_LOAD_LOCAL_INDEX) };
            let literal = unsafe { &*site.add(CONDITION_RIGHT_OPERAND_SITE_INDEX) };
            let compare = unsafe { &*site.add(CONDITION_COMPARE_INDEX) };
            let branch = unsafe { &*site.add(CONDITION_BRANCH_INDEX) };
            let local_in_bounds = load.left < frame_ref.local_count;
            let local = if local_in_bounds {
                unsafe { *frame_ref.local_values.add(load.left) }
            } else {
                RawValue(FIRST_HEAP_TAG)
            };
            let literal_value = RawValue(literal.literal);
            let load_destination = unsafe { frame_ref.register_values.add(load.dst) };
            let literal_destination = unsafe { frame_ref.register_values.add(literal.dst) };
            let compare_destination = unsafe { frame_ref.register_values.add(compare.dst) };
            let guard = local_in_bounds
                && is_number(local)
                && is_number(literal_value)
                && !needs_reference_count(unsafe { *load_destination })
                && !needs_reference_count(unsafe { *literal_destination })
                && !needs_reference_count(unsafe { *compare_destination });
            if !guard {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let result = f64::from_bits(local.0) $operator f64::from_bits(literal_value.0);
            let comparison = RawValue(BOOL_TAG | if result { TRUE_PAYLOAD } else { FALSE_PAYLOAD });
            unsafe {
                *load_destination = local;
                *literal_destination = literal_value;
                *compare_destination = comparison;
            }
            if result {
                unsafe { become __quench_dyn_hole_next(frame, site.add(CONDITION_INSTRUCTION_COUNT)) }
            } else {
                let target_site = unsafe { frame_ref.sites.add(branch.literal as usize) };
                unsafe { become __quench_dyn_hole_branch(frame, target_site) }
            }
        }
    };
}

define_dyn_local_literal_condition!(quench_dyn_condition_local_literal_less, <);
define_dyn_local_literal_condition!(quench_dyn_condition_local_literal_less_equal, <=);
define_dyn_local_literal_condition!(quench_dyn_condition_local_literal_greater, >);
define_dyn_local_literal_condition!(quench_dyn_condition_local_literal_greater_equal, >=);

macro_rules! finish_dead_branch {
    ($frame:ident, $frame_ref:ident, $site:ident, $result:ident, $branch_index:expr, $instruction_count:expr) => {
        if $result {
            unsafe {
                become __quench_dyn_hole_next($frame, $site.add($instruction_count))
            }
        } else {
            let branch = unsafe { &*$site.add($branch_index) };
            let target_site = unsafe { $frame_ref.sites.add(branch.literal as usize) };
            unsafe { become __quench_dyn_hole_branch($frame, target_site) }
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_dead_cached_instanceof_condition(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &*frame };
    let result = match unsafe { (frame_ref.instanceof_condition)(frame, site) } {
        INSTANCEOF_CACHE_FALSE => false,
        INSTANCEOF_CACHE_TRUE => true,
        _ => unsafe { become __quench_dyn_hole_slow(frame, site) },
    };
    finish_dead_branch!(
        frame,
        frame_ref,
        site,
        result,
        INSTANCEOF_BRANCH_INDEX,
        INSTANCEOF_INSTRUCTION_COUNT
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_dead_cached_instanceof_not_condition(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &*frame };
    let membership = match unsafe { (frame_ref.instanceof_condition)(frame, site) } {
        INSTANCEOF_CACHE_FALSE => false,
        INSTANCEOF_CACHE_TRUE => true,
        _ => unsafe { become __quench_dyn_hole_slow(frame, site) },
    };
    let result = !membership;
    finish_dead_branch!(
        frame,
        frame_ref,
        site,
        result,
        INSTANCEOF_NOT_BRANCH_INDEX,
        INSTANCEOF_NOT_INSTRUCTION_COUNT
    );
}

#[inline(always)]
fn nullish_equal(left: RawValue, _right: RawValue) -> bool {
    let tag = left.0 & TAG_MASK;
    tag == NULL_TAG || tag == UNDEFINED_TAG
}

#[inline(always)]
fn nullish_not_equal(left: RawValue, right: RawValue) -> bool {
    !nullish_equal(left, right)
}

#[inline(always)]
fn immediate_strict_equal(left: RawValue, right: RawValue) -> bool {
    left.0 == right.0
}

#[inline(always)]
fn immediate_strict_not_equal(left: RawValue, right: RawValue) -> bool {
    !immediate_strict_equal(left, right)
}

macro_rules! define_dyn_dead_own_property_literal_condition {
    ($name:ident, $predicate:path) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &*frame };
            let load = unsafe { &*site };
            if load.left >= frame_ref.local_count {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let receiver = unsafe { *frame_ref.local_values.add(load.left) };
            let property = unsafe {
                read_cached_property(receiver, site.add(PROPERTY_LITERAL_GET_INDEX))
            };
            let Some(property) = property else {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            };
            let literal = unsafe { RawValue((*site.add(PROPERTY_LITERAL_LITERAL_INDEX)).literal) };
            let result = $predicate(property, literal);
            finish_dead_branch!(
                frame,
                frame_ref,
                site,
                result,
                PROPERTY_LITERAL_BRANCH_INDEX,
                PROPERTY_LITERAL_INSTRUCTION_COUNT
            );
        }
    };
}

define_dyn_dead_own_property_literal_condition!(
    quench_dyn_dead_own_property_nullish_equal,
    nullish_equal
);
define_dyn_dead_own_property_literal_condition!(
    quench_dyn_dead_own_property_nullish_not_equal,
    nullish_not_equal
);
define_dyn_dead_own_property_literal_condition!(
    quench_dyn_dead_own_property_immediate_strict_equal,
    immediate_strict_equal
);
define_dyn_dead_own_property_literal_condition!(
    quench_dyn_dead_own_property_immediate_strict_not_equal,
    immediate_strict_not_equal
);

macro_rules! define_dyn_dead_number_condition {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(CONDITION_LOAD_LOCAL_INDEX) };
            let literal = unsafe { &*site.add(CONDITION_RIGHT_OPERAND_SITE_INDEX) };
            let local_in_bounds = load.left < frame_ref.local_count;
            let local = if local_in_bounds {
                unsafe { *frame_ref.local_values.add(load.left) }
            } else {
                RawValue(FIRST_HEAP_TAG)
            };
            let literal_value = RawValue(literal.literal);
            if !local_in_bounds || !is_number(local) || !is_number(literal_value) {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let result = f64::from_bits(local.0) $operator f64::from_bits(literal_value.0);
            finish_dead_branch!(
                frame,
                frame_ref,
                site,
                result,
                CONDITION_BRANCH_INDEX,
                CONDITION_INSTRUCTION_COUNT
            );
        }
    };
}

define_dyn_dead_number_condition!(quench_dyn_dead_condition_local_number_less, <);
define_dyn_dead_number_condition!(quench_dyn_dead_condition_local_number_less_equal, <=);
define_dyn_dead_number_condition!(quench_dyn_dead_condition_local_number_greater, >);
define_dyn_dead_number_condition!(quench_dyn_dead_condition_local_number_greater_equal, >=);
define_dyn_dead_number_condition!(quench_dyn_dead_condition_local_number_equal, ==);
define_dyn_dead_number_condition!(quench_dyn_dead_condition_local_number_not_equal, !=);

macro_rules! define_dyn_dead_immediate_strict_condition {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(CONDITION_LOAD_LOCAL_INDEX) };
            if load.left >= frame_ref.local_count {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let literal = unsafe { &*site.add(CONDITION_RIGHT_OPERAND_SITE_INDEX) };
            let local = unsafe { *frame_ref.local_values.add(load.left) };
            let result = local.0 $operator literal.literal;
            finish_dead_branch!(
                frame,
                frame_ref,
                site,
                result,
                CONDITION_BRANCH_INDEX,
                CONDITION_INSTRUCTION_COUNT
            );
        }
    };
}

define_dyn_dead_immediate_strict_condition!(
    quench_dyn_dead_condition_local_immediate_strict_equal,
    ==
);
define_dyn_dead_immediate_strict_condition!(
    quench_dyn_dead_condition_local_immediate_strict_not_equal,
    !=
);

macro_rules! define_dyn_dead_nullish_condition {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(CONDITION_LOAD_LOCAL_INDEX) };
            if load.left >= frame_ref.local_count {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let local = unsafe { *frame_ref.local_values.add(load.left) };
            let local_tag = local.0 & TAG_MASK;
            let is_nullish = local_tag == NULL_TAG || local_tag == UNDEFINED_TAG;
            let result = is_nullish $operator true;
            finish_dead_branch!(
                frame,
                frame_ref,
                site,
                result,
                CONDITION_BRANCH_INDEX,
                CONDITION_INSTRUCTION_COUNT
            );
        }
    };
}

define_dyn_dead_nullish_condition!(quench_dyn_dead_condition_local_nullish_equal, ==);
define_dyn_dead_nullish_condition!(quench_dyn_dead_condition_local_nullish_not_equal, !=);

macro_rules! define_dyn_dead_name_number_condition {
    ($name:ident, $compare_operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(NAME_CONDITION_LOAD_LOCAL_INDEX) };
            let name_site = unsafe { &*site.add(NAME_CONDITION_LOAD_NAME_INDEX) };
            let local_in_bounds = load.left < frame_ref.local_count;
            let snapshots_available = !frame_ref.name_snapshots.is_null();
            if !local_in_bounds || !snapshots_available {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let local = unsafe { *frame_ref.local_values.add(load.left) };
            let snapshot_slot = name_site.literal as usize;
            let named = unsafe { *frame_ref.name_snapshots.add(snapshot_slot) };
            if !is_number(local) || !is_number(named) {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let result = f64::from_bits(local.0) $compare_operator f64::from_bits(named.0);
            finish_dead_branch!(
                frame,
                frame_ref,
                site,
                result,
                NAME_CONDITION_BRANCH_INDEX,
                NAME_CONDITION_INSTRUCTION_COUNT
            );
        }
    };
}

define_dyn_dead_name_number_condition!(quench_dyn_dead_condition_local_name_number_equal, ==);
define_dyn_dead_name_number_condition!(quench_dyn_dead_condition_local_name_number_not_equal, !=);
define_dyn_dead_name_number_condition!(quench_dyn_dead_condition_local_name_number_less, <);
define_dyn_dead_name_number_condition!(quench_dyn_dead_condition_local_name_number_less_equal, <=);
define_dyn_dead_name_number_condition!(quench_dyn_dead_condition_local_name_number_greater, >);
define_dyn_dead_name_number_condition!(
    quench_dyn_dead_condition_local_name_number_greater_equal,
    >=
);

macro_rules! define_dyn_dead_update_jump {
    ($name:ident, $update_operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(UPDATE_LOAD_LOCAL_INDEX) };
            let literal = unsafe { &*site.add(UPDATE_LITERAL_INDEX) };
            let store = unsafe { &*site.add(UPDATE_STORE_LOCAL_INDEX) };
            let jump = unsafe { &*site.add(UPDATE_JUMP_INDEX) };
            let slots_match = load.left == store.dst;
            let local_in_bounds = load.left < frame_ref.local_count;
            let local = if local_in_bounds {
                unsafe { *frame_ref.local_values.add(load.left) }
            } else {
                RawValue(FIRST_HEAP_TAG)
            };
            let literal_value = RawValue(literal.literal);
            if !slots_match
                || !local_in_bounds
                || !is_number(local)
                || !is_number(literal_value)
            {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let result = canonical_number(
                f64::from_bits(local.0) $update_operator f64::from_bits(literal_value.0),
            );
            unsafe { *frame_ref.local_values.add(store.dst) = result };
            let target_site = unsafe { frame_ref.sites.add(jump.literal as usize) };
            unsafe { become __quench_dyn_hole_next(frame, target_site) }
        }
    };
}

define_dyn_dead_update_jump!(quench_dyn_dead_update_local_number_add_jump, +);
define_dyn_dead_update_jump!(quench_dyn_dead_update_local_number_subtract_jump, -);

macro_rules! define_dyn_dead_recurrence {
    ($name:ident, $update_operator:tt, $compare_operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(frame: *mut RawDynFrame, site: *const InlineSite) {
            let frame_ref = unsafe { &mut *frame };
            let load = unsafe { &*site.add(UPDATE_LOAD_LOCAL_INDEX) };
            let update_literal = unsafe { &*site.add(UPDATE_LITERAL_INDEX) };
            let store = unsafe { &*site.add(UPDATE_STORE_LOCAL_INDEX) };
            let bound_literal = unsafe { &*site.add(RECURRENCE_BOUND_LITERAL_INDEX) };
            let slots_match = load.left == store.dst;
            let local_in_bounds = load.left < frame_ref.local_count;
            let local = if local_in_bounds {
                unsafe { *frame_ref.local_values.add(load.left) }
            } else {
                RawValue(FIRST_HEAP_TAG)
            };
            let update_value = RawValue(update_literal.literal);
            let bound_value = RawValue(bound_literal.literal);
            if !slots_match
                || !local_in_bounds
                || !is_number(local)
                || !is_number(update_value)
                || !is_number(bound_value)
            {
                unsafe { become __quench_dyn_hole_slow(frame, site) }
            }
            let result = canonical_number(
                f64::from_bits(local.0) $update_operator f64::from_bits(update_value.0),
            );
            unsafe { *frame_ref.local_values.add(store.dst) = result };
            let comparison = f64::from_bits(result.0) $compare_operator f64::from_bits(bound_value.0);
            finish_dead_branch!(
                frame,
                frame_ref,
                site,
                comparison,
                RECURRENCE_BRANCH_INDEX,
                RECURRENCE_INSTRUCTION_COUNT
            );
        }
    };
}

macro_rules! define_dyn_dead_recurrence_family {
    (
        $update_operator:tt,
        $equal:ident,
        $not_equal:ident,
        $less:ident,
        $less_equal:ident,
        $greater:ident,
        $greater_equal:ident
    ) => {
        define_dyn_dead_recurrence!($equal, $update_operator, ==);
        define_dyn_dead_recurrence!($not_equal, $update_operator, !=);
        define_dyn_dead_recurrence!($less, $update_operator, <);
        define_dyn_dead_recurrence!($less_equal, $update_operator, <=);
        define_dyn_dead_recurrence!($greater, $update_operator, >);
        define_dyn_dead_recurrence!($greater_equal, $update_operator, >=);
    };
}

define_dyn_dead_recurrence_family!(
    +,
    quench_dyn_dead_recurrence_add_equal,
    quench_dyn_dead_recurrence_add_not_equal,
    quench_dyn_dead_recurrence_add_less,
    quench_dyn_dead_recurrence_add_less_equal,
    quench_dyn_dead_recurrence_add_greater,
    quench_dyn_dead_recurrence_add_greater_equal
);
define_dyn_dead_recurrence_family!(
    -,
    quench_dyn_dead_recurrence_subtract_equal,
    quench_dyn_dead_recurrence_subtract_not_equal,
    quench_dyn_dead_recurrence_subtract_less,
    quench_dyn_dead_recurrence_subtract_less_equal,
    quench_dyn_dead_recurrence_subtract_greater,
    quench_dyn_dead_recurrence_subtract_greater_equal
);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_jump(frame: *mut RawDynFrame, site: *const InlineSite) {
    let frame_ref = unsafe { &*frame };
    let target_pc = unsafe { (*site).literal as usize };
    let target_site = unsafe { frame_ref.sites.add(target_pc) };
    unsafe { become __quench_dyn_hole_next(frame, target_site) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_dyn_jump_if_false(
    frame: *mut RawDynFrame,
    site: *const InlineSite,
) {
    let frame_ref = unsafe { &*frame };
    let site_ref = unsafe { &*site };
    let test = unsafe { *frame_ref.register_values.add(site_ref.left) };
    match immediate_truthiness(test) {
        TRUTHINESS_TRUE => unsafe { become __quench_dyn_hole_next(frame, site.add(1)) },
        TRUTHINESS_FALSE => {
            let target_site = unsafe { frame_ref.sites.add(site_ref.literal as usize) };
            unsafe { become __quench_dyn_hole_branch(frame, target_site) }
        }
        _ => unsafe { become __quench_dyn_hole_slow(frame, site) },
    }
}

macro_rules! define_number_binary_stencil {
    ($name:ident, $operator:tt) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            thread: *mut u8,
            frame: *mut RawValue,
            code: *const u8,
            site: *mut u8,
            accumulator: RawValue,
            tag_mask: u64,
            first_tag: u64,
            right_bits: u64,
        ) -> RawValue {
            let left = f64::from_bits(accumulator.0);
            let right = f64::from_bits(right_bits);
            let result = RawValue((left $operator right).to_bits());
            unsafe {
                become __quench_hole_next(
                    thread, frame, code, site, result, tag_mask, first_tag, right_bits,
                )
            }
        }
    };
}

define_number_binary_stencil!(quench_number_add, +);
define_number_binary_stencil!(quench_number_subtract, -);
define_number_binary_stencil!(quench_number_multiply, *);
define_number_binary_stencil!(quench_number_divide, /);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn quench_load_value(
    thread: *mut u8,
    frame: *mut RawValue,
    code: *const u8,
    site: *mut u8,
    _accumulator: RawValue,
    tag_mask: u64,
    first_tag: u64,
    value_bits: u64,
) -> RawValue {
    let result = RawValue(value_bits);
    unsafe {
        become __quench_hole_next(
            thread, frame, code, site, result, tag_mask, first_tag, value_bits,
        )
    }
}
