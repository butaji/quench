//! Calls the lane answers in place for their common case (else `native_call_miss`), and
//! for-in's step (`fast_names_step`, the loop exit's row).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::call_complete::{concrete_returns_callee_completion, concrete_write_call_result};
use quench_runtime::staged::strings::{ConcreteString, concrete_char_at, concrete_char_code_at};
use quench_runtime::staged::Concrete;
use quench_runtime::vm::object_get::ordinary_chain::ChainRead;

/// `charCodeAt` of a flat string at an integer index (`staged::builtins::char_code_at`): the
/// unit there, or NaN outside the string. `None` for any other receiver or index, which the
/// native coerces, and for a rope, which it flattens.
#[inline(always)]
pub(crate) fn string_char_code_at(vm: &Vm, receiver: Value, index: Value) -> Option<Value> {
    let records = vm.heap.string_record_base().unwrap_or(std::ptr::null());
    concrete_char_code_at(ConcreteString::Record { records, value: receiver }, index)
        .or_else(|| concrete_char_code_at(ConcreteString::Units(vm.heap.flat_text(receiver)?.units()), index))
}

/// `charAt` of a flat string at an integer index of an ASCII unit
/// (`staged::builtins::char_at_ascii`): the unit's shared string (`Vm::ascii_byte_string`).
/// Any other receiver, index or unit takes the native.
pub(crate) fn string_char_at_ascii(vm: &mut Vm, receiver: Value, index: Value) -> Option<Value> {
    let (records, table) = (vm.heap.string_record_base().unwrap_or(std::ptr::null()), vm.heap.ascii_unit_strings_base().unwrap_or(std::ptr::null()));
    if let Some(character) = concrete_char_at(ConcreteString::Record { records, value: receiver }, table, index) {
        return Some(character);
    }
    concrete_char_at(ConcreteString::Units(vm.heap.flat_text(receiver)?.units()), table, index)
}

/// The natives whose steady case (`Vm::steady_native_value`) a `Call` site makes under its own
/// handler, one instantiation of `builtin_miss` per row: the native is a constant of the
/// handler, so its entry is a direct call and the site tests only the callee's identity.
macro_rules! builtin_rows {
    ($($row:literal => $native:ident),* $(,)?) => {
        const fn row_native(row: u8) -> Native {
            match row { $($row => Native::$native,)* _ => unreachable!() }
        }
        fn builtin_row(native: Native) -> Option<u8> {
            match native { $(Native::$native => Some($row),)* _ => None }
        }
        /// The protocol row of a Call of builtin row `row` (`native_call_handler`).
        const fn builtin_handler(row: u8) -> Handler {
            match row { $($row => protocol_handler::<{ quench_runtime::staged::forms::PLAIN_CALL_ROW }, ACC_NONE, { protocol_family::LEAF_CALL | (BUILTIN_KIND | $row) as u16 }>(),)* _ => call_row::<{ protocol_family::NATIVE_CALL | protocol_misses::native_call::NATIVE as u16 }>() }
        }
        /// The miss body of builtin row `row` (`leaf_call_miss_of`).
        pub(crate) const fn builtin_miss_of(row: u8) -> Handler {
            match row { $($row => builtin_miss::<$row> as Handler,)* _ => native_call_miss }
        }
        /// The builtin rows, whose identity slots come first (`NativeIdentities`).
        const BUILTIN_ROWS: usize = [$($row),*].len();
    };
}
builtin_rows!(
    0 => MapGet, 1 => MapHas, 2 => MapSet, 3 => SetHas, 4 => WeakMapGet, 5 => WeakMapHas,
    6 => WeakSetHas, 7 => StringCharAt, 8 => ArrayIsArray, 9 => StringCharCodeAt, 10 => ArrayPush, 11 => ArrayPop,
    12 => JsonStringify, 13 => DateNow, 14 => RegExpTest, 15 => RegExpExec,
);


/// The Math functions whose call a site makes under its own handler, one instantiation of
/// `math_native_miss` per row: the native is a constant, so the site tests the callee's
/// identity and runs the native's staged row (`math_row_call`) with no dispatch on the native. Every other inline Math native keeps `call_math_miss`.
macro_rules! math_rows {
    ($($row:literal => $native:ident),* $(,)?) => {
        const fn math_row_native(row: u8) -> Native {
            match row { $($row => Native::$native,)* _ => unreachable!() }
        }
        fn math_handler(native: Native) -> Option<Handler> {
            match native { $(Native::$native => Some(protocol_handler::<{ quench_runtime::staged::forms::PLAIN_CALL_ROW }, ACC_NONE, { protocol_family::LEAF_CALL | (MATH_KIND | $row) as u16 }>()),)* _ => None }
        }
        /// The miss body of Math row `row` (`leaf_call_miss_of`).
        pub(crate) const fn math_miss_of(row: u8) -> Handler {
            match row { $($row => math_native_miss::<$row> as Handler,)* _ => call_math_miss }
        }
        /// The Math rows, whose identity slots follow the builtin rows' (`NativeIdentities`).
        const MATH_ROWS: usize = [$($row),*].len();
    };
}
math_rows!(
    0 => MathSqrt, 1 => MathAbs, 2 => MathFloor, 3 => MathCeil, 4 => MathRound, 5 => MathTrunc,
    6 => MathMin, 7 => MathMax, 8 => MathImul, 9 => MathFround, 10 => MathSign,
);

/// The staged row of Math row `row` (`number::math_call`), which every Math row has.
const fn math_row_call(row: u8) -> quench_runtime::staged::call_native::MathCall {
    match super::number::math_call(math_row_native(row)) {
        Some(call) => call,
        None => panic!("a Math row runs a staged row"),
    }
}

use quench_runtime::vm::engine_types::MAX_IN_PLACE_ROWS;

/// No row (`IN_PLACE_ROW_OF`).
const NO_IN_PLACE_ROW: u16 = u16::MAX;

/// The rows' natives by discriminant, and how many there are.
const IN_PLACE_ROW_NATIVES: ([u16; MAX_IN_PLACE_ROWS], usize) = {
    let mut natives = [0; MAX_IN_PLACE_ROWS];
    let (mut index, mut rows) = (0, 0);
    while index < Native::COUNT {
        if super::in_place_natives::runs_no_guest_code(Native::from_discriminant(index).unwrap()) {
            natives[rows] = index;
            rows += 1;
        }
        index += 1;
    }
    (natives, rows)
};

/// The lean rows, whose identity slots follow the Math rows' (`NativeIdentities`).
const IN_PLACE_ROWS: usize = IN_PLACE_ROW_NATIVES.1;
const _: () = assert!(IN_PLACE_ROWS <= MAX_IN_PLACE_ROWS, "raise MAX_IN_PLACE_ROWS and the handler arms");

/// Each native's lean row, `NO_IN_PLACE_ROW` for one that runs guest code.
const IN_PLACE_ROW_OF: [u16; Native::COUNT as usize] = {
    let mut rows = [NO_IN_PLACE_ROW; Native::COUNT as usize];
    let mut row = 0;
    while row < IN_PLACE_ROWS {
        rows[IN_PLACE_ROW_NATIVES.0[row] as usize] = row as u16;
        row += 1;
    }
    rows
};

/// The native of lean row `row`. A row past the derived ones names a native with no in-place
/// family, whose handler is never installed and folds to its miss.
const fn in_place_row_native(row: u8) -> Native {
    if (row as usize) < IN_PLACE_ROWS {
        match Native::from_discriminant(IN_PLACE_ROW_NATIVES.0[row as usize]) {
            Some(native) => native,
            None => Native::MathInitialize,
        }
    } else {
        Native::MathInitialize
    }
}

/// The lean row handler of `native`, where it runs no guest code.
fn in_place_handler(native: Native) -> Option<Handler> {
    let row = IN_PLACE_ROW_OF[native as usize];
    if row == NO_IN_PLACE_ROW {
        return None;
    }
    Some(match row as u8 {
            0 => lane_call_in_place::<0>,
            1 => lane_call_in_place::<1>,
            2 => lane_call_in_place::<2>,
            3 => lane_call_in_place::<3>,
            4 => lane_call_in_place::<4>,
            5 => lane_call_in_place::<5>,
            6 => lane_call_in_place::<6>,
            7 => lane_call_in_place::<7>,
            8 => lane_call_in_place::<8>,
            9 => lane_call_in_place::<9>,
            10 => lane_call_in_place::<10>,
            11 => lane_call_in_place::<11>,
            12 => lane_call_in_place::<12>,
            13 => lane_call_in_place::<13>,
            14 => lane_call_in_place::<14>,
            15 => lane_call_in_place::<15>,
            16 => lane_call_in_place::<16>,
            17 => lane_call_in_place::<17>,
            18 => lane_call_in_place::<18>,
            19 => lane_call_in_place::<19>,
            20 => lane_call_in_place::<20>,
            21 => lane_call_in_place::<21>,
            22 => lane_call_in_place::<22>,
            23 => lane_call_in_place::<23>,
            24 => lane_call_in_place::<24>,
            25 => lane_call_in_place::<25>,
            26 => lane_call_in_place::<26>,
            27 => lane_call_in_place::<27>,
            28 => lane_call_in_place::<28>,
            29 => lane_call_in_place::<29>,
            30 => lane_call_in_place::<30>,
            31 => lane_call_in_place::<31>,
            32 => lane_call_in_place::<32>,
            33 => lane_call_in_place::<33>,
            34 => lane_call_in_place::<34>,
            35 => lane_call_in_place::<35>,
            36 => lane_call_in_place::<36>,
            37 => lane_call_in_place::<37>,
            38 => lane_call_in_place::<38>,
            39 => lane_call_in_place::<39>,
            40 => lane_call_in_place::<40>,
            41 => lane_call_in_place::<41>,
            42 => lane_call_in_place::<42>,
            43 => lane_call_in_place::<43>,
            44 => lane_call_in_place::<44>,
            45 => lane_call_in_place::<45>,
            46 => lane_call_in_place::<46>,
            47 => lane_call_in_place::<47>,
            48 => lane_call_in_place::<48>,
            49 => lane_call_in_place::<49>,
            50 => lane_call_in_place::<50>,
            51 => lane_call_in_place::<51>,
            52 => lane_call_in_place::<52>,
            53 => lane_call_in_place::<53>,
            54 => lane_call_in_place::<54>,
            55 => lane_call_in_place::<55>,
            56 => lane_call_in_place::<56>,
            57 => lane_call_in_place::<57>,
            58 => lane_call_in_place::<58>,
            59 => lane_call_in_place::<59>,
            60 => lane_call_in_place::<60>,
            61 => lane_call_in_place::<61>,
            62 => lane_call_in_place::<62>,
            63 => lane_call_in_place::<63>,
            64 => lane_call_in_place::<64>,
            65 => lane_call_in_place::<65>,
            66 => lane_call_in_place::<66>,
            67 => lane_call_in_place::<67>,
            68 => lane_call_in_place::<68>,
            69 => lane_call_in_place::<69>,
            70 => lane_call_in_place::<70>,
            71 => lane_call_in_place::<71>,
            72 => lane_call_in_place::<72>,
            73 => lane_call_in_place::<73>,
            74 => lane_call_in_place::<74>,
            75 => lane_call_in_place::<75>,
            76 => lane_call_in_place::<76>,
            77 => lane_call_in_place::<77>,
            78 => lane_call_in_place::<78>,
            79 => lane_call_in_place::<79>,
            80 => lane_call_in_place::<80>,
            81 => lane_call_in_place::<81>,
            82 => lane_call_in_place::<82>,
            83 => lane_call_in_place::<83>,
            84 => lane_call_in_place::<84>,
            85 => lane_call_in_place::<85>,
            86 => lane_call_in_place::<86>,
            87 => lane_call_in_place::<87>,
            88 => lane_call_in_place::<88>,
            89 => lane_call_in_place::<89>,
            90 => lane_call_in_place::<90>,
            91 => lane_call_in_place::<91>,
            92 => lane_call_in_place::<92>,
            93 => lane_call_in_place::<93>,
            94 => lane_call_in_place::<94>,
            95 => lane_call_in_place::<95>,
            96 => lane_call_in_place::<96>,
            97 => lane_call_in_place::<97>,
            98 => lane_call_in_place::<98>,
            99 => lane_call_in_place::<99>,
            100 => lane_call_in_place::<100>,
            101 => lane_call_in_place::<101>,
            102 => lane_call_in_place::<102>,
            103 => lane_call_in_place::<103>,
            104 => lane_call_in_place::<104>,
            105 => lane_call_in_place::<105>,
            106 => lane_call_in_place::<106>,
            107 => lane_call_in_place::<107>,
            108 => lane_call_in_place::<108>,
            109 => lane_call_in_place::<109>,
            110 => lane_call_in_place::<110>,
            111 => lane_call_in_place::<111>,
            112 => lane_call_in_place::<112>,
            113 => lane_call_in_place::<113>,
            114 => lane_call_in_place::<114>,
            115 => lane_call_in_place::<115>,
            116 => lane_call_in_place::<116>,
            117 => lane_call_in_place::<117>,
            118 => lane_call_in_place::<118>,
            119 => lane_call_in_place::<119>,
            120 => lane_call_in_place::<120>,
            121 => lane_call_in_place::<121>,
            122 => lane_call_in_place::<122>,
            123 => lane_call_in_place::<123>,
            124 => lane_call_in_place::<124>,
            125 => lane_call_in_place::<125>,
            126 => lane_call_in_place::<126>,
            127 => lane_call_in_place::<127>,
            128 => lane_call_in_place::<128>,
            129 => lane_call_in_place::<129>,
            130 => lane_call_in_place::<130>,
            131 => lane_call_in_place::<131>,
            132 => lane_call_in_place::<132>,
            133 => lane_call_in_place::<133>,
            134 => lane_call_in_place::<134>,
            135 => lane_call_in_place::<135>,
            136 => lane_call_in_place::<136>,
            137 => lane_call_in_place::<137>,
            138 => lane_call_in_place::<138>,
            139 => lane_call_in_place::<139>,
            140 => lane_call_in_place::<140>,
            141 => lane_call_in_place::<141>,
            142 => lane_call_in_place::<142>,
            143 => lane_call_in_place::<143>,
            144 => lane_call_in_place::<144>,
            145 => lane_call_in_place::<145>,
            146 => lane_call_in_place::<146>,
            147 => lane_call_in_place::<147>,
            148 => lane_call_in_place::<148>,
            149 => lane_call_in_place::<149>,
            150 => lane_call_in_place::<150>,
            151 => lane_call_in_place::<151>,
            152 => lane_call_in_place::<152>,
            153 => lane_call_in_place::<153>,
            154 => lane_call_in_place::<154>,
            155 => lane_call_in_place::<155>,
            156 => lane_call_in_place::<156>,
            157 => lane_call_in_place::<157>,
            158 => lane_call_in_place::<158>,
            159 => lane_call_in_place::<159>,
            160 => lane_call_in_place::<160>,
            161 => lane_call_in_place::<161>,
            162 => lane_call_in_place::<162>,
            163 => lane_call_in_place::<163>,
            164 => lane_call_in_place::<164>,
            165 => lane_call_in_place::<165>,
            166 => lane_call_in_place::<166>,
            167 => lane_call_in_place::<167>,
            168 => lane_call_in_place::<168>,
            169 => lane_call_in_place::<169>,
            170 => lane_call_in_place::<170>,
            171 => lane_call_in_place::<171>,
            172 => lane_call_in_place::<172>,
            173 => lane_call_in_place::<173>,
            174 => lane_call_in_place::<174>,
            175 => lane_call_in_place::<175>,
            176 => lane_call_in_place::<176>,
            177 => lane_call_in_place::<177>,
            178 => lane_call_in_place::<178>,
            179 => lane_call_in_place::<179>,
            180 => lane_call_in_place::<180>,
            181 => lane_call_in_place::<181>,
            182 => lane_call_in_place::<182>,
            183 => lane_call_in_place::<183>,
            184 => lane_call_in_place::<184>,
            185 => lane_call_in_place::<185>,
            186 => lane_call_in_place::<186>,
            187 => lane_call_in_place::<187>,
            188 => lane_call_in_place::<188>,
            189 => lane_call_in_place::<189>,
            190 => lane_call_in_place::<190>,
            191 => lane_call_in_place::<191>,
            192 => lane_call_in_place::<192>,
            193 => lane_call_in_place::<193>,
            194 => lane_call_in_place::<194>,
            195 => lane_call_in_place::<195>,
            196 => lane_call_in_place::<196>,
            197 => lane_call_in_place::<197>,
            198 => lane_call_in_place::<198>,
            199 => lane_call_in_place::<199>,
            200 => lane_call_in_place::<200>,
            201 => lane_call_in_place::<201>,
            202 => lane_call_in_place::<202>,
            203 => lane_call_in_place::<203>,
            204 => lane_call_in_place::<204>,
            205 => lane_call_in_place::<205>,
            206 => lane_call_in_place::<206>,
            207 => lane_call_in_place::<207>,
            208 => lane_call_in_place::<208>,
            209 => lane_call_in_place::<209>,
            210 => lane_call_in_place::<210>,
            211 => lane_call_in_place::<211>,
            212 => lane_call_in_place::<212>,
            213 => lane_call_in_place::<213>,
            214 => lane_call_in_place::<214>,
            215 => lane_call_in_place::<215>,
            216 => lane_call_in_place::<216>,
            217 => lane_call_in_place::<217>,
            218 => lane_call_in_place::<218>,
            219 => lane_call_in_place::<219>,
            220 => lane_call_in_place::<220>,
            221 => lane_call_in_place::<221>,
            222 => lane_call_in_place::<222>,
            223 => lane_call_in_place::<223>,
            224 => lane_call_in_place::<224>,
            225 => lane_call_in_place::<225>,
            226 => lane_call_in_place::<226>,
            227 => lane_call_in_place::<227>,
            228 => lane_call_in_place::<228>,
            229 => lane_call_in_place::<229>,
            230 => lane_call_in_place::<230>,
            231 => lane_call_in_place::<231>,
            232 => lane_call_in_place::<232>,
            233 => lane_call_in_place::<233>,
            234 => lane_call_in_place::<234>,
            235 => lane_call_in_place::<235>,
            236 => lane_call_in_place::<236>,
            237 => lane_call_in_place::<237>,
            238 => lane_call_in_place::<238>,
            239 => lane_call_in_place::<239>,
            240 => lane_call_in_place::<240>,
            241 => lane_call_in_place::<241>,
            242 => lane_call_in_place::<242>,
            243 => lane_call_in_place::<243>,
            244 => lane_call_in_place::<244>,
            245 => lane_call_in_place::<245>,
            246 => lane_call_in_place::<246>,
            247 => lane_call_in_place::<247>,
            248 => lane_call_in_place::<248>,
            249 => lane_call_in_place::<249>,
            250 => lane_call_in_place::<250>,
            251 => lane_call_in_place::<251>,
            252 => lane_call_in_place::<252>,
            253 => lane_call_in_place::<253>,
            254 => lane_call_in_place::<254>,
            255 => lane_call_in_place::<255>
        })
}

use quench_runtime::vm::engine_types::IDENTITY_SLOTS;
const _: () = assert!(BUILTIN_ROWS + MATH_ROWS + IN_PLACE_ROWS == IDENTITY_SLOTS, "set engine_types::IDENTITY_SLOTS to the rows");

/// The identity slot of lean row `row`. A row past the derived ones (`in_place_row_native`),
/// never installed, reads the first slot, so its handler stays in bounds.
const fn in_place_slot(row: u8) -> usize {
    if (row as usize) < IN_PLACE_ROWS { BUILTIN_ROWS + MATH_ROWS + row as usize } else { 0 }
}

/// The identity slot of Math row `row`.
const fn math_slot(row: u8) -> usize {
    BUILTIN_ROWS + row as usize
}





/// Whether `callee` is `native`, the native of identity slot `slot`: the row's admission, one
/// compare of the callee word against the identity the lane proved
/// (`staged::call_native::admit_identity`), else the proof from the callee's record, made once a
/// generation (`prove_native_identity`).
#[inline(always)]
fn native_admitted(vm: &mut Vm, callee: Value, slot: usize, native: Native) -> bool {
    quench_runtime::staged::call_native::concrete_admit_identity(callee.to_bits(), vm.lane_stack.native_identities.0[slot])
        || prove_native_identity(vm, callee, slot, native)
}

/// `native_admitted` past its compare: whether `callee`'s record is `native`'s, which the slot
/// then holds until the generation ends.
#[cold]
#[inline(never)]
fn prove_native_identity(vm: &mut Vm, callee: Value, slot: usize, native: Native) -> bool {
    let proved = callee_is_native(vm, callee, native);
    if proved {
        vm.lane_stack.native_identities.0[slot] = callee.to_bits();
    }
    proved
}

/// The natives whose in-place body is called directly (`in_place_row_value`), not through the
/// family table of `Vm::in_place_native_value` (`in_place_natives`' `ROW` family). Their lean
/// rows are the derived ones (`in_place_handler`), like every other in-place native's.
macro_rules! in_place_rows {
    ($($row:literal => $native:ident),* $(,)?) => {
        /// The in-place row of `native`, or `None` where no row serves it (`in_place_natives`).
        pub(crate) const fn in_place_row(native: Native) -> Option<u8> {
            match native { $(Native::$native => Some($row),)* _ => None }
        }
    };
}
in_place_rows!(
    0 => ArraySlice, 1 => ArrayConcat, 2 => ArrayIndexOf, 3 => ArrayIncludes, 4 => ArrayJoin,
    5 => ArrayAt, 6 => ArrayFill, 7 => ObjectKeys, 8 => ObjectAssign,
    9 => StringTrim, 10 => StringIndexOf, 11 => StringSlice, 12 => StringSplit,
    13 => StringToUpperCase, 14 => StringToLowerCase, 15 => StringPadStart,
    16 => StringStartsWith, 17 => StringIncludes,
);

/// The handlers of a Math call's field read: `GetField` rows (`generated_row`) at the three Math call
/// sites (`MATH_CALL_SITE`, `MATH_CALL2_SITE`, `MATH_CALL2_CONST_SITE`). `FieldHit` reads a record
/// of these as an own-field read.
pub(crate) fn math_field_call_handlers() -> [*const (); 3] {
    [generated_row!(FIELD_LEAF_READ, ACC_NONE, MATH_CALL_SITE) as *const (),
        generated_row!(FIELD_LEAF_READ, ACC_NONE, MATH_CALL2_SITE) as *const (),
        generated_row!(FIELD_LEAF_READ, ACC_NONE, MATH_CALL2_CONST_SITE) as *const ()]
}

script_handler! {
    /// A `Call` site whose callee was the Math function of row `ROW` (`native_call_handler`): the
    /// native's admission (`native_admitted`) and its staged row of Number arguments
    /// (`staged::call_native::math_call`). Any other callee or argument takes `call_math_miss`.
    pub(crate) fn math_native_miss<ROW: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let native = const { math_row_native(ROW) };
        let value = if native_admitted(vm, r.get(i.b), const { math_slot(ROW) }, native) {
            // SAFETY: the call's window is registers of the running function.
            quench_runtime::staged::call_native::concrete_math_call(const { math_row_call(ROW) }, unsafe { call_window_in_lane(r, i) })
        } else {
            None
        };
        let Some(value) = value else {
            sibling!(call_math_miss, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// The rest of the Math call whose callee the field read `ip` (math site `SITE`) produced: the
/// argument copies and the call, as the records write them (the fused handler's three writes). The
/// callee's value is computed by the VM's Math function (`PropertyFrame::math_call`); a call the
/// frame does not compute continues at the record after the read, which makes the call as before.
/// Returns the record the row continues at.
#[inline(always)]
pub(crate) fn math_row_resume<F: quench_runtime::staged::PropertyFrame<quench_runtime::staged::Concrete>, const SITE: u8>(
    frame: &mut F, stage: &mut quench_runtime::staged::Concrete, r: Registers, ip: Ip, callee: u64) -> Ip {
    let first_ip = following(ip);
    // SAFETY: the installer matched the copies and the `Call` that follow in this view.
    unsafe {
        if const { math_arity(SITE) == 1 } {
            let (copy, call_ip) = (&*first_ip, following(first_ip));
            let call = &*call_ip;
            let argument = r.get(copy.b);
            let Some(value) = frame.math_call(stage, callee, [argument.to_bits(), 0], 1) else { return first_ip };
            r.set(copy.a, argument);
            r.set(call.a, Value::from_bits(value));
            return following(call_ip);
        }
        let (second_ip, call_ip) = (following(first_ip), following(following(first_ip)));
        let (first, second, call) = (&*first_ip, &*second_ip, &*call_ip);
        let second_value = if const { SITE == MATH_CALL2_CONST_SITE } { second.constant() } else { r.get(second.b) };
        let arguments = [r.get(first.b), second_value];
        let Some(value) = frame.math_call(stage, callee, [arguments[0].to_bits(), arguments[1].to_bits()], 2) else { return first_ip };
        r.set(first.a, arguments[0]);
        r.set(second.a, arguments[1]);
        r.set(call.a, Value::from_bits(value));
        following(call_ip)
    }
}

/// Fuses the call `call_ip`, which `native_call_handler` just gave Math row handler `handler`,
/// with the `GetField` of its callee and the `Move` of its one argument before it: the field
/// record's row (`MATH_CALL_SITE`) runs the read and the call (`math_row_resume`). The callee
/// register is the field's own read (`own_bound_handlers`), the copy a plain `Move` into the
/// call's one-value window.
pub(crate) fn install_math_field_call(cx: *mut LaneContext, call_ip: Ip, native: Native) {
    if math_handler(native).is_none() { return }
    // SAFETY: `call_ip` is a record of the running view, which starts at the context's base.
    let pc = unsafe { call_ip.offset_from((*cx).base) };
    if pc < 2 {
        return;
    }
    // SAFETY: as below; a two-argument call has its field record three before it.
    let call = unsafe { &*call_ip };
    if ImmediateLayout::argument_count(call.imm()) == 2 {
        if pc >= 3 {
            install_math_field_call2(call_ip);
        }
        return;
    }
    // SAFETY: both records precede the call in the same view.
    let (copy_ip, field_ip) = unsafe { (call_ip.sub(1), call_ip.sub(2)) };
    let (call, copy, field) = unsafe { (&*call_ip, &*copy_ip, &*field_ip) };
    let one_argument = ImmediateLayout::argument_count(call.imm()) == 1
        && ImmediateLayout::call_window_base(call.imm()) == copy.a;
    let copies = copy.handler == generated_row!(quench_runtime::staged::MOVE_ROW, ACC_NONE, FLAG_UNREAD) as Handler as *const ();
    let reads_callee = field.a == call.b
        && own_bound_handlers()[1..].iter().step_by(2).any(|&handler| handler == field.handler);
    if one_argument && copies && reads_callee && copy.a != field.a {
        requicken(field_ip, generated_row!(FIELD_LEAF_READ, ACC_NONE, MATH_CALL_SITE) as Handler);
    }
}

/// `install_math_field_call` of a two-argument call: a field record, a `Move` of the first
/// argument and a `Move` or `LoadConst` of the second into the call's two-value window.
fn install_math_field_call2(call_ip: Ip) {
    // SAFETY: the three records precede the call in the same view (the caller checked).
    let (second_ip, first_ip, field_ip) = unsafe { (call_ip.sub(1), call_ip.sub(2), call_ip.sub(3)) };
    let (call, second, first, field) = unsafe { (&*call_ip, &*second_ip, &*first_ip, &*field_ip) };
    let window = ImmediateLayout::call_window_base(call.imm());
    let copy = generated_row!(quench_runtime::staged::MOVE_ROW, ACC_NONE, FLAG_UNREAD) as Handler as *const ();
    let constant_second = second.handler == generated_row!(quench_runtime::staged::LOAD_CONST_ROW, ACC_NONE, FLAG_UNREAD) as Handler as *const ();
    let shaped = first.handler == copy && (second.handler == copy || constant_second)
        && first.a == window && second.a == window + 1;
    let reads_callee = field.a == call.b && field.a != window && field.a != window + 1
        && own_bound_handlers()[1..].iter().step_by(2).any(|&handler| handler == field.handler);
    if shaped && reads_callee {
        let row = if constant_second { generated_row!(FIELD_LEAF_READ, ACC_NONE, MATH_CALL2_CONST_SITE) as Handler }
            else { generated_row!(FIELD_LEAF_READ, ACC_NONE, MATH_CALL2_SITE) as Handler };
        requicken(field_ip, row);
    }
}

/// The steady case of builtin row `ROW`, which calls its native's one body directly. The row's
/// native is a constant, so each `if const` arm not taken is dropped and no arm re-matches the
/// native at run time. Every row of `builtin_rows` has its arm here; a native without one declines.
#[inline(always)]
fn builtin_row_value<const ROW: u8>(vm: &mut Vm, receiver: Value, args: &[Value]) -> Option<Value> {
    // A new attempt clears any flag a previous attempt left for its general native.
    vm.declined_fast = None;
    let first = args.first().copied();
    if const { matches!(row_native(ROW), Native::MapGet | Native::MapHas) } {
        let found = vm.pure_collection_lookup(const { row_native(ROW) }, receiver, first.unwrap_or(Value::UNDEFINED));
        return vm.fast_declined(const { row_native(ROW) }, found);
    }
    if const { matches!(row_native(ROW), Native::SetHas | Native::WeakMapGet | Native::WeakMapHas | Native::WeakSetHas) } {
        return vm.pure_collection_lookup(const { row_native(ROW) }, receiver, first.unwrap_or(Value::UNDEFINED));
    }
    if const { matches!(row_native(ROW), Native::MapSet) } {
        return if vm.map_set(receiver, args) { Some(receiver) } else { None };
    }
    if const { matches!(row_native(ROW), Native::ArrayPush) } {
        return vm.array_push_dense(receiver, args);
    }
    if const { matches!(row_native(ROW), Native::ArrayPop) } {
        return vm.array_pop_dense(receiver);
    }
    if const { matches!(row_native(ROW), Native::StringCharCodeAt) } {
        return vm.string_char_code_at_core(receiver, first?);
    }
    if const { matches!(row_native(ROW), Native::StringCharAt) } {
        return vm.string_char_at_core(receiver, first?);
    }
    if const { matches!(row_native(ROW), Native::ArrayIsArray) } {
        return vm.steady_is_array(first.unwrap_or(Value::UNDEFINED)).map(Value::boolean);
    }
    if const { matches!(row_native(ROW), Native::JsonStringify) } {
        let text = vm.json_stringify_steady(args);
        return vm.fast_declined(const { row_native(ROW) }, text);
    }
    if const { matches!(row_native(ROW), Native::DateNow) } {
        return Some(vm.date_now());
    }
    None
}

script_handler! {
    /// A `Call` site whose callee was the native of builtin row `ROW` (`native_call_handler`): the
    /// native's admission (`native_admitted`, the staged `admit_identity`), then that call in its
    /// steady case (`Vm::steady_native_value`). Any other callee or case takes
    /// `native_call_miss`.
    pub(crate) fn builtin_miss<ROW: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let native = const { row_native(ROW) };
        let value = if !native_admitted(vm, r.get(i.b), ROW as usize, native) {
            None
        } else if const { matches!(row_native(ROW), Native::RegExpTest | Native::RegExpExec) } {
            // A RegExp method's steady call (`Vm::steady_regexp_call`), entered from the site
            // without the general native protocol; an error it raises completes as
            // `native_call_miss`'s does.
            // SAFETY: the call's window is registers of the running function; the context's
            // program outlives the lane run.
            let (args, code) = unsafe { (call_window_in_lane(r, i), &*(*cx).code) };
            let result_used = ImmediateLayout::call_result_used_in_lane(i.imm());
            match args.first().and_then(|&subject| vm.steady_regexp_call(code, native, r.get(i.c), subject, result_used)) {
                Some(Ok(value)) => Some(value),
                Some(Err(error)) => {
                    note_allocation(vm);
                    vm.lane_completion.raise(error);
                    resume!(cx, native_complete_in_lane(cx, ip, i, None))
                }
                None => None,
            }
        } else {
            // SAFETY: the call's window is registers of the running function.
            vm.steady_native_value(native, r.get(i.c), unsafe { call_window_in_lane(r, i) })
        };
        let Some(value) = value else {
            sibling!(native_call_miss, r, ip, Carried::script(cx))
        };
        // A steady case may allocate; a due collection runs at the lane's next safepoint.
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The in-place body of an `in_place_rows` native, called directly with its receiver and
/// arguments. Each arm is the body `Vm::in_place_native_value` reaches for that native, with
/// the same arguments and the same preconditions. `None` leaves the call to the native.
#[inline(always)]
pub(crate) fn in_place_row_value(vm: &mut Vm, code: &ResidualProgram, native: Native, receiver: Value,
    args: &[Value]) -> Option<Value> {
    // A new attempt clears any flag a previous attempt left for its general native.
    vm.declined_fast = None;
    let first = args.first().copied().unwrap_or(Value::UNDEFINED);
    match native {
        Native::ArraySlice => {
            let sliced = vm.plain_array_slice(receiver, args);
            vm.fast_declined(native, sliced)
        }
        Native::ArrayConcat => {
            let joined = vm.array_concat_dense(receiver, args);
            vm.fast_declined(native, joined)
        }
        Native::ArrayIndexOf | Native::ArrayIncludes => {
            let found = vm.dense_search_resume(native, receiver, args);
            vm.fast_search_declined(native, found)
        }
        Native::ArrayJoin => vm.plain_array_join(receiver, args),
        Native::ArrayAt => vm.plain_array_at(receiver, args),
        Native::ArrayFill => vm.plain_array_fill(receiver, args),
        Native::ObjectKeys => {
            let listed = vm.plain_enumerable_own_properties(first, quench_runtime::vm::object_keys::EnumerableOwnPropertyKind::Key);
            vm.fast_declined(native, listed)
        }
        Native::ObjectAssign => {
            let copied = vm.plain_object_assign(args);
            vm.fast_declined(native, copied)
        }
        // Each string row's body, folded to it by `pure_string_method_call`'s inlining.
        Native::StringSplit => {
            let parts = vm.pure_string_method_call(code, native, receiver, args);
            vm.fast_declined(native, parts)
        }
        Native::StringTrim | Native::StringIndexOf | Native::StringSlice
        | Native::StringToUpperCase | Native::StringToLowerCase | Native::StringPadStart
        | Native::StringStartsWith | Native::StringIncludes => vm.pure_string_method_call(code, native, receiver, args),
        _ => None,
    }
}

script_handler! {
    /// A `Call` site whose callee was the native of lean row `ROW` (`in_place_rows`): the
    /// native's admission (`native_admitted`, one compare of the callee word), then its in-place
    /// body over the call's window, read where it lies, and the value stored to the call's
    /// register: the steps `native_call_miss` reaches through `native_value_in_lane` for this
    /// native, in the same order, without its dispatch. Any other callee or case takes
    /// `native_call_miss`, which tries the same body again and then calls the native.
    fn lane_call_in_place<ROW: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let native = const { in_place_row_native(ROW) };
        let call = quench_runtime::staged::call_native::InPlaceCall {
            callee: i.b,
            receiver: i.c,
            arguments: ImmediateLayout::call_window_base(i.imm()),
            result: i.a,
        };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let count = usize::from(ImmediateLayout::argument_count(i.imm()));
        let registers = r.0.expose_provenance() as u64;
        let slot = const { in_place_slot(ROW) };
        let mut stage = Concrete::new(std::ptr::null());
        // The shared in-place call step (`call_native::step_in_place_call_of`, the b-jit's builtin rows'):
        // the callee admitted against the identity the lane proved, the body, the result stored.
        let identity = vm.lane_stack.native_identities.0[slot];
        let mut body = InPlaceLane { vm: &mut *vm, code, native, callee: r.get(i.b), count };
        let mut done = quench_runtime::staged::call_native::step_in_place_call_of(&mut stage, &mut body, registers, identity, call, ());
        // A callee the slot does not hold yet: proved once (`prove_native_identity`), then the step again.
        if done.is_none() && r.get(i.b).to_bits() != identity && prove_native_identity(vm, r.get(i.b), slot, native) {
            let mut body = InPlaceLane { vm: &mut *vm, code, native, callee: r.get(i.b), count };
            done = quench_runtime::staged::call_native::step_in_place_call_of(&mut stage, &mut body, registers, r.get(i.b).to_bits(), call, ());
        }
        if done.is_none() {
            sibling!(native_call_miss, r, ip, Carried::script(cx))
        }
        // An in-place body may allocate; a due collection runs at the lane's next safepoint.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The interpreter's in-place body of `native` (`call_native::InPlaceBody`): `Vm::in_place_native_value`
/// of `callee` over the call's window of `count` values where it lies, a `DataView` float64 access first.
struct InPlaceLane<'a> {
    vm: &'a mut Vm,
    code: &'a ResidualProgram,
    native: Native,
    callee: Value,
    count: usize,
}

impl quench_runtime::staged::call_native::InPlaceBody<Concrete> for InPlaceLane<'_> {
    #[inline(always)]
    fn in_place_value(&mut self, _stage: &mut Concrete, receiver: u64, arguments: u64, _miss: ()) -> Option<u64> {
        // SAFETY: the call's window is registers of the running function, which no in-place body moves.
        let args = unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<Value>(arguments as usize), self.count) };
        let (vm, native, receiver) = (&mut *self.vm, self.native, Value::from_bits(receiver));
        let float64 = if matches!(native, Native::DataViewGetFloat64 | Native::DataViewSetFloat64) {
            let value = vm.data_view_access_of(native, receiver, args);
            if value.is_some() && vm.execution_mode == quench_runtime::vm::ExecutionMode::Compiled {
                vm.programs.note_js_jit_dataview_float64(native == Native::DataViewSetFloat64);
            }
            value
        } else {
            None
        };
        float64.or_else(|| vm.in_place_native_value_inline(self.code, native, self.callee, receiver, args)).map(Value::to_bits)
    }
}

/// The handler a `Call` site whose callee resolved to `native` takes from then on
/// (`script_call_resolve_miss`): a native whose common case the lane makes in place has its
/// own handler, which tests the callee's identity once and hands any other call to
/// `native_call_miss`, the handler of every other native. A call returning its result
/// completes its caller, which only `native_call_miss` makes. An array callback method's site
/// whose result nothing reads (`result_dead`, asked only there) takes the form that may
/// discard it (`call_array_callback_miss`'s `DISCARDS`). A `String.prototype.replace` whose
/// result only the `GetField "length"` after it reads (`length_only`, asked only there) takes
/// the replacement's length without building it (`regexp_calls::replace_length_handler`).
pub(crate) fn native_call_handler(native: Native, i: &LaneInstruction, result_dead: impl FnOnce() -> bool, length_only: impl FnOnce() -> bool) -> Handler {
    if concrete_returns_callee_completion(i.a) {
        return call_row::<{ protocol_family::NATIVE_CALL | protocol_misses::native_call::NATIVE as u16 }>();
    }
    match native {
        native if let Some(row) = builtin_row(native) => builtin_handler(row),
        native if let Some(handler) = math_handler(native) => handler,
        Native::StringReplace if length_only() => super::regexp_calls::replace_length_handler(),
        native if let Some(handler) = in_place_handler(native) => handler,
        native if super::number::math_numbers_inline(native) => protocol_handler::<{ quench_runtime::staged::forms::PLAIN_CALL_ROW }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::CALL_MATH as u16 }>(),
        native if native.is_array_callback() && result_dead() => protocol_handler::<{ quench_runtime::staged::forms::PLAIN_CALL_ROW }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::CALL_ARRAY_CALLBACK_DISCARDING as u16 }>(),
        native if native.is_array_callback() => protocol_handler::<{ quench_runtime::staged::forms::PLAIN_CALL_ROW }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::CALL_ARRAY_CALLBACK as u16 }>(),
        _ => call_row::<{ protocol_family::NATIVE_CALL | protocol_misses::native_call::NATIVE as u16 }>(),
    }
}

/// A lane body's call of a native: one that completes in place (`Vm::in_place_native_value`: no
/// guest code, no raised error, no frame observed) writes its value to the call's register, which
/// the body reads as it reads a completed script call's, so the body stays native across
/// `Math.imul`, a string method or a collection read. Any other native is called out
/// (`body_native_out_in_lane`). A call that returns its result is a miss, which leaves the body at the call.
pub(crate) fn body_native_call_in_lane(cx: *mut LaneContext, call: &super::body_forms::BodyCall, callee: Value, receiver: Value) -> u64 {
    use crate::compiled::LANE_CALL_COMPLETED;
    let vm = unsafe { vm(cx) };
    let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = vm.heap.function(callee) else { return super::lane_body_stats::miss(vm, super::lane_body_stats::Miss::NativeNotNative) };
    let native = *native;
    // SAFETY: the context's program outlives the lane run; the values are registers of the
    // running function, which the native reads before anything can move them.
    let (code, args) = unsafe { (&*(*cx).code, call.values()) };
    let Some(value) = vm.in_place_native_value(code, native, callee, receiver, &args) else {
        // A returning call the native makes out of line is the lane's to complete.
        if call.site.returns { return super::lane_body_stats::miss(vm, super::lane_body_stats::Miss::Returning); }
        return body_native_out_in_lane(cx, call, native, (callee, receiver), &args);
    };
    // A returning call's value completes the body: it reads the call's register, as it does a
    // returning script call's (`Lowering::return_register`, `steps::completing`).
    note_allocation(vm);
    concrete_write_call_result(call.registers.0.expose_provenance() as u64, call.site.result, value);
    LANE_CALL_COMPLETED
}

script_handler! {
    /// `native_call_miss` of `Reflect.get`, read once along an ordinary chain
    /// (`Vm::reflect_chain_read`): a data value or an absent key completes the call in place,
    /// an accessor with a getter is that getter's activation with the receiver argument as
    /// `this` (`activate_getter_in_lane`), whose completion is the call's value; any other
    /// call is the native's (`native_call_value_in_lane`).
    #[inline(never)]
    pub(crate) fn reflect_get_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the window is registers of the running function, read before anything runs.
        let window = unsafe { call_window_in_lane(r, i) };
        if concrete_returns_callee_completion(i.a) && !returning_call_admitted(cx) {
            return exit(ip);
        }
        let read = match window {
            [target, key, ..] => vm.reflect_chain_read(*target, *key).map(|read| (read, window.get(2).copied().unwrap_or(*target))),
            _ => None,
        };
        match read {
            Some((ChainRead::Value(value), _)) => resume!(cx, native_complete_in_lane(cx, ip, i, Some(value))),
            Some((ChainRead::Absent, _)) => resume!(cx, native_complete_in_lane(cx, ip, i, Some(Value::UNDEFINED))),
            Some((ChainRead::Getter(Some(getter)), receiver)) if !getter.is_undefined() => {
                match activate_getter_in_lane(cx, r, ip, getter, receiver, None) {
                    Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                    Err(next) => next,
                }
            }
            _ => resume!(cx, native_call_value_in_lane(cx, r, ip, i, Native::ReflectGet)),
        }
    }
}

/// The handler of a `Reflect.get` call site (`native_call_miss`'s sibling).
pub(crate) fn reflect_get_call_handler() -> Handler {
    protocol_handler::<{ quench_runtime::staged::forms::PLAIN_CALL_ROW }, ACC_NONE, { protocol_family::LEAF_CALL | REFLECT_GET_KIND as u16 }>()
}

/// `body_native_call_in_lane` of a native that is not in place: the lane's own call of it
/// (`call_native_in_lane`), beside the body's activations where the native allows, else from the
/// frame, so the callbacks it makes run as activations of the lane above the body
/// (`Vm::deferred_callback_in_lane`, `Vm::native_callback_in_lane`) and their returns go back to the
/// native, which returns to the body. `LANE_CALL_COMPLETED` with the value in the call's
/// register; `LANE_CALL_LEFT` where the native made the body's activations frames, whose
/// registers then hold it and which continue at the next record; `LANE_CALL_MISSED` where the
/// native is not one the lane calls or raised (the error stashed in `Vm::lane_raised` for the
/// general path, as `lane_body_value_step`'s).
fn body_native_out_in_lane(cx: *mut LaneContext, call: &super::body_forms::BodyCall, native: Native,
    (callee, receiver): (Value, Value), args: &[Value]) -> u64 {
    use crate::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT};
    let ip = call.ip;
    // An array callback method's callbacks are activations of the lane (`begin_array_iteration_in_lane`).
    if native.is_array_callback() && let Some(status) = body_array_iteration_in_lane(cx, ip, native, receiver, args) {
        return status;
    }
    let Some(value) = call_native_in_lane(cx, ip, native, callee, receiver, args) else {
        return super::lane_body_stats::miss(unsafe { vm(cx) }, super::lane_body_stats::Miss::NativeNotCompleted);
    };
    // Guest code the native ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    concrete_write_call_result(after.0.expose_provenance() as u64, call.site.result, value);
    if after.0 == call.registers.0 { return LANE_CALL_COMPLETED; }
    unsafe { (*cx).callee_resume = following(ip); (*cx).callee_acc = Value::UNDEFINED; }
    LANE_CALL_LEFT
}
