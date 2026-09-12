use super::model::{NumericBinary, NumericUnary, PropertyValueKind, QuotedLoop, RegionOp};
use crate::dynbytecode::Register;
use crate::u32_js;
use std::collections::{BTreeMap, BTreeSet};

pub const REGISTER_REGION_WORD_LANE_COUNT: usize = 4;
pub const REGISTER_REGION_F64_LANE_COUNT: usize = 4;
const MIN_REGISTER_REGION_NUMERIC_OPERATIONS: usize = 4;
const SINGLE_LOCATION_OWNER: usize = 1;

pub type RegisterLane = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Word32Kind {
    Signed,
    Unsigned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegisterLocation {
    Word32 {
        lane: RegisterLane,
        kind: Word32Kind,
    },
    F64(RegisterLane),
}

impl RegisterLocation {
    pub const fn lane(self) -> RegisterLane {
        match self {
            Self::Word32 { lane, .. } | Self::F64(lane) => lane,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterConversion {
    F64ToWord32,
    SignedWord32ToF64,
    UnsignedWord32ToF64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisterRegionStep {
    Nop {
        pc: usize,
    },
    CopyLoadLocal {
        pc: usize,
    },
    LoadLocal {
        pc: usize,
        destination: RegisterLane,
    },
    LoadWordLocal {
        pc: usize,
        destination: RegisterLane,
    },
    LoadLiteral {
        pc: usize,
        destination: RegisterLane,
    },
    LoadWordLiteral {
        pc: usize,
        destination: RegisterLane,
        word: u32,
    },
    LoadName {
        pc: usize,
        destination: RegisterLane,
    },
    CopyLoadName {
        pc: usize,
    },
    StoreLocal {
        pc: usize,
        source: RegisterLane,
    },
    CopyStoreLocal {
        pc: usize,
    },
    Move {
        pc: usize,
        destination: RegisterLocation,
        source: RegisterLocation,
    },
    CopyMove {
        pc: usize,
    },
    Convert {
        pc: usize,
        destination: RegisterLane,
        source: RegisterLane,
        kind: RegisterConversion,
    },
    Unary {
        pc: usize,
        destination: RegisterLocation,
        source: RegisterLocation,
        kind: NumericUnary,
    },
    Binary {
        pc: usize,
        destination: RegisterLocation,
        left: RegisterLocation,
        right: RegisterLocation,
        kind: NumericBinary,
    },
    CompareBranch {
        pc: usize,
        left: RegisterLane,
        right: RegisterLane,
        kind: NumericBinary,
        target: usize,
    },
    ReadDense {
        pc: usize,
        destination: RegisterLane,
        index: RegisterLane,
    },
    ReadDenseWord {
        pc: usize,
        destination: RegisterLane,
        index: RegisterLane,
    },
    WriteDense {
        pc: usize,
        index: RegisterLane,
        source: RegisterLane,
    },
    LoadStatic {
        pc: usize,
        destination: RegisterLane,
    },
    CopyReadStatic {
        pc: usize,
    },
    WriteStatic {
        pc: usize,
        source: RegisterLane,
    },
    CopyWriteStatic {
        pc: usize,
    },
    Jump {
        pc: usize,
        target: usize,
    },
}

impl RegisterRegionStep {
    pub const fn pc(&self) -> usize {
        match self {
            Self::Nop { pc }
            | Self::CopyLoadLocal { pc }
            | Self::LoadLocal { pc, .. }
            | Self::LoadWordLocal { pc, .. }
            | Self::LoadLiteral { pc, .. }
            | Self::LoadWordLiteral { pc, .. }
            | Self::LoadName { pc, .. }
            | Self::CopyLoadName { pc }
            | Self::StoreLocal { pc, .. }
            | Self::CopyStoreLocal { pc }
            | Self::Move { pc, .. }
            | Self::CopyMove { pc }
            | Self::Convert { pc, .. }
            | Self::Unary { pc, .. }
            | Self::Binary { pc, .. }
            | Self::CompareBranch { pc, .. }
            | Self::ReadDense { pc, .. }
            | Self::ReadDenseWord { pc, .. }
            | Self::WriteDense { pc, .. }
            | Self::LoadStatic { pc, .. }
            | Self::CopyReadStatic { pc }
            | Self::WriteStatic { pc, .. }
            | Self::CopyWriteStatic { pc }
            | Self::Jump { pc, .. } => *pc,
        }
    }

    pub const fn bytecode_width(&self) -> usize {
        match self {
            Self::Convert { .. } => 0,
            Self::CompareBranch { .. } => 2,
            _ => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterRegionPlan {
    pub start: usize,
    pub end: usize,
    pub exit: usize,
    pub steps: Box<[RegisterRegionStep]>,
    pub maximum_live_word_lanes: usize,
    pub maximum_live_f64_lanes: usize,
    pub numeric_operations: usize,
    pub conversions: usize,
    pub forwarded_local_loads: usize,
    pub alias_updates: usize,
    pub maximum_location_fanout: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisterRegionReject {
    NotSingleTrace,
    TooLittleNumericWork,
    UnsupportedBooleanUse {
        pc: usize,
    },
    MissingNumericValue {
        pc: usize,
        register: Register,
    },
    RegisterPressure {
        pc: usize,
        word_lanes: usize,
        f64_lanes: usize,
    },
    LiveValuesAtBackedge {
        pc: usize,
        count: usize,
    },
}

#[derive(Default)]
struct TemporaryLanes {
    words: BTreeSet<RegisterLane>,
    numbers: BTreeSet<RegisterLane>,
    converted: BTreeMap<RegisterLocation, RegisterLocation>,
}

pub fn plan_register_region(
    quote: &QuotedLoop,
) -> Result<RegisterRegionPlan, RegisterRegionReject> {
    if quote
        .trace_header()
        .is_some_and(|header| header != quote.start)
    {
        return Err(RegisterRegionReject::NotSingleTrace);
    }
    let operations = quote.operations();
    let numeric = numeric_registers(quote, &operations);
    let boolean_results = operations
        .iter()
        .filter_map(|operation| match operation {
            RegionOp::Binary { dst, kind, .. } if kind.result_is_boolean() => Some(*dst),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut remaining = numeric_use_counts(
        &operations,
        &numeric,
        &boolean_results,
        quote.internal_local_sources(),
    );
    let mut locations = BTreeMap::<Register, RegisterLocation>::new();
    let mut steps = Vec::new();
    let mut maximum_live_word_lanes = 0;
    let mut maximum_live_f64_lanes = 0;
    let mut numeric_operations = 0;
    let mut conversions = 0;
    let mut forwarded_local_loads = 0;
    let mut alias_updates = 0;
    let mut maximum_location_fanout = SINGLE_LOCATION_OWNER;
    let mut index = 0;

    while index < operations.len() {
        let operation = &operations[index];
        let pc = operation_pc(operation);
        let mut temporaries = TemporaryLanes::default();
        let step = match operation {
            RegionOp::Elided { .. } => RegisterRegionStep::Nop { pc },
            RegionOp::NumberLiteral { dst, bits, .. } => {
                let word_literal = value_has_only_word32_uses(*dst, &operations);
                let destination_shape = if word_literal {
                    RegisterLocation::Word32 {
                        lane: 0,
                        kind: Word32Kind::Signed,
                    }
                } else {
                    RegisterLocation::F64(0)
                };
                let destination = allocate_destination(
                    pc,
                    *dst,
                    destination_shape,
                    &locations,
                    &mut maximum_live_word_lanes,
                    &mut maximum_live_f64_lanes,
                )?
                .lane();
                if word_literal {
                    locations.insert(
                        *dst,
                        RegisterLocation::Word32 {
                            lane: destination,
                            kind: Word32Kind::Signed,
                        },
                    );
                    RegisterRegionStep::LoadWordLiteral {
                        pc,
                        destination,
                        word: u32_js(f64::from_bits(*bits)),
                    }
                } else {
                    locations.insert(*dst, RegisterLocation::F64(destination));
                    RegisterRegionStep::LoadLiteral { pc, destination }
                }
            }
            RegionOp::ReadLocal { dst, .. }
                if numeric.contains(dst) && quote.internal_local_sources().contains_key(dst) =>
            {
                let source = quote.internal_local_sources()[dst];
                let source_location = consume(pc, source, &mut remaining, &mut locations)?;
                locations.insert(*dst, source_location);
                forwarded_local_loads += 1;
                alias_updates += 1;
                maximum_location_fanout =
                    maximum_location_fanout.max(location_fanout(source_location, &locations));
                RegisterRegionStep::Nop { pc }
            }
            RegionOp::ReadLocal { dst, .. } if numeric.contains(dst) => {
                let word_local = value_has_only_word32_uses(*dst, &operations);
                let destination_shape = if word_local {
                    RegisterLocation::Word32 {
                        lane: 0,
                        kind: Word32Kind::Signed,
                    }
                } else {
                    RegisterLocation::F64(0)
                };
                let destination = allocate_destination(
                    pc,
                    *dst,
                    destination_shape,
                    &locations,
                    &mut maximum_live_word_lanes,
                    &mut maximum_live_f64_lanes,
                )?
                .lane();
                if word_local {
                    locations.insert(
                        *dst,
                        RegisterLocation::Word32 {
                            lane: destination,
                            kind: Word32Kind::Signed,
                        },
                    );
                    RegisterRegionStep::LoadWordLocal { pc, destination }
                } else {
                    locations.insert(*dst, RegisterLocation::F64(destination));
                    RegisterRegionStep::LoadLocal { pc, destination }
                }
            }
            RegionOp::ReadLocal { .. } => RegisterRegionStep::CopyLoadLocal { pc },
            RegionOp::ReadCaptured { dst, .. } if numeric.contains(dst) => {
                let destination = allocate_destination(
                    pc,
                    *dst,
                    RegisterLocation::F64(0),
                    &locations,
                    &mut maximum_live_word_lanes,
                    &mut maximum_live_f64_lanes,
                )?
                .lane();
                locations.insert(*dst, RegisterLocation::F64(destination));
                RegisterRegionStep::LoadName { pc, destination }
            }
            RegionOp::ReadCaptured { .. } => RegisterRegionStep::CopyLoadName { pc },
            RegionOp::WriteLocal { src, .. } if numeric.contains(src) => {
                let source = prepare_f64(
                    pc,
                    *src,
                    &locations,
                    &mut temporaries,
                    &mut steps,
                    &mut conversions,
                )?;
                consume(pc, *src, &mut remaining, &mut locations)?;
                if locations.contains_key(src) {
                    // The store already paid the representation change.  Keep
                    // that exact F64 value as the source of a later
                    // dataflow-proven local read instead of converting the
                    // old Word32 representation a second time.
                    locations.insert(*src, source);
                }
                RegisterRegionStep::StoreLocal {
                    pc,
                    source: source.lane(),
                }
            }
            RegionOp::WriteLocal { .. } => RegisterRegionStep::CopyStoreLocal { pc },
            RegionOp::Move { dst, src, .. } if numeric.contains(dst) => {
                let source = location(pc, *src, &locations)?;
                consume(pc, *src, &mut remaining, &mut locations)?;
                locations.insert(*dst, source);
                alias_updates += 1;
                maximum_location_fanout =
                    maximum_location_fanout.max(location_fanout(source, &locations));
                RegisterRegionStep::Nop { pc }
            }
            RegionOp::Move { .. } => RegisterRegionStep::CopyMove { pc },
            RegionOp::Unary { dst, src, kind, .. } => {
                numeric_operations += 1;
                let (source, destination_shape) = match kind {
                    NumericUnary::Plus | NumericUnary::Negate => (
                        prepare_f64(
                            pc,
                            *src,
                            &locations,
                            &mut temporaries,
                            &mut steps,
                            &mut conversions,
                        )?,
                        RegisterLocation::F64(0),
                    ),
                    NumericUnary::BitNot => (
                        prepare_word32(
                            pc,
                            *src,
                            &locations,
                            &mut temporaries,
                            &mut steps,
                            &mut conversions,
                        )?,
                        RegisterLocation::Word32 {
                            lane: 0,
                            kind: Word32Kind::Signed,
                        },
                    ),
                };
                consume(pc, *src, &mut remaining, &mut locations)?;
                let destination = allocate_destination(
                    pc,
                    *dst,
                    destination_shape,
                    &locations,
                    &mut maximum_live_word_lanes,
                    &mut maximum_live_f64_lanes,
                )?;
                locations.insert(*dst, destination);
                RegisterRegionStep::Unary {
                    pc,
                    destination,
                    source,
                    kind: *kind,
                }
            }
            RegionOp::Binary {
                dst,
                left,
                right,
                kind,
                ..
            } if kind.result_is_boolean() => {
                let Some(RegionOp::JumpIfFalse { test, target, .. }) = operations.get(index + 1)
                else {
                    return Err(RegisterRegionReject::UnsupportedBooleanUse { pc });
                };
                if test != dst {
                    return Err(RegisterRegionReject::UnsupportedBooleanUse { pc });
                }
                let left_location = prepare_f64(
                    pc,
                    *left,
                    &locations,
                    &mut temporaries,
                    &mut steps,
                    &mut conversions,
                )?;
                let right_location = prepare_f64(
                    pc,
                    *right,
                    &locations,
                    &mut temporaries,
                    &mut steps,
                    &mut conversions,
                )?;
                if left_location == right_location {
                    return Err(RegisterRegionReject::UnsupportedBooleanUse { pc });
                }
                consume(pc, *left, &mut remaining, &mut locations)?;
                consume(pc, *right, &mut remaining, &mut locations)?;
                numeric_operations += 1;
                index += 1;
                RegisterRegionStep::CompareBranch {
                    pc,
                    left: left_location.lane(),
                    right: right_location.lane(),
                    kind: *kind,
                    target: *target,
                }
            }
            RegionOp::Binary {
                dst,
                left,
                right,
                kind,
                ..
            } => {
                let word_result = word32_result(*kind);
                let left_location = if word_result.is_some() {
                    prepare_word32(
                        pc,
                        *left,
                        &locations,
                        &mut temporaries,
                        &mut steps,
                        &mut conversions,
                    )?
                } else {
                    prepare_f64(
                        pc,
                        *left,
                        &locations,
                        &mut temporaries,
                        &mut steps,
                        &mut conversions,
                    )?
                };
                let right_location = if word_result.is_some() {
                    prepare_word32(
                        pc,
                        *right,
                        &locations,
                        &mut temporaries,
                        &mut steps,
                        &mut conversions,
                    )?
                } else {
                    prepare_f64(
                        pc,
                        *right,
                        &locations,
                        &mut temporaries,
                        &mut steps,
                        &mut conversions,
                    )?
                };
                consume(pc, *left, &mut remaining, &mut locations)?;
                consume(pc, *right, &mut remaining, &mut locations)?;
                let destination_shape = match word_result {
                    Some(kind) => RegisterLocation::Word32 { lane: 0, kind },
                    None => RegisterLocation::F64(0),
                };
                let destination = allocate_destination(
                    pc,
                    *dst,
                    destination_shape,
                    &locations,
                    &mut maximum_live_word_lanes,
                    &mut maximum_live_f64_lanes,
                )?;
                locations.insert(*dst, destination);
                numeric_operations += 1;
                RegisterRegionStep::Binary {
                    pc,
                    destination,
                    left: left_location,
                    right: right_location,
                    kind: *kind,
                }
            }
            RegionOp::ReadDense {
                dst, index: source, ..
            } => {
                let index_location = prepare_f64(
                    pc,
                    *source,
                    &locations,
                    &mut temporaries,
                    &mut steps,
                    &mut conversions,
                )?;
                consume(pc, *source, &mut remaining, &mut locations)?;
                let word_result = value_has_only_word32_uses(*dst, &operations);
                let destination_shape = if word_result {
                    RegisterLocation::Word32 {
                        lane: 0,
                        kind: Word32Kind::Signed,
                    }
                } else {
                    RegisterLocation::F64(0)
                };
                let destination = allocate_destination(
                    pc,
                    *dst,
                    destination_shape,
                    &locations,
                    &mut maximum_live_word_lanes,
                    &mut maximum_live_f64_lanes,
                )?
                .lane();
                if word_result {
                    locations.insert(
                        *dst,
                        RegisterLocation::Word32 {
                            lane: destination,
                            kind: Word32Kind::Signed,
                        },
                    );
                    RegisterRegionStep::ReadDenseWord {
                        pc,
                        destination,
                        index: index_location.lane(),
                    }
                } else {
                    locations.insert(*dst, RegisterLocation::F64(destination));
                    RegisterRegionStep::ReadDense {
                        pc,
                        destination,
                        index: index_location.lane(),
                    }
                }
            }
            RegionOp::WriteDense {
                index: key, src, ..
            } => {
                let index_location = prepare_f64(
                    pc,
                    *key,
                    &locations,
                    &mut temporaries,
                    &mut steps,
                    &mut conversions,
                )?;
                let source_location = prepare_f64(
                    pc,
                    *src,
                    &locations,
                    &mut temporaries,
                    &mut steps,
                    &mut conversions,
                )?;
                consume(pc, *key, &mut remaining, &mut locations)?;
                consume(pc, *src, &mut remaining, &mut locations)?;
                RegisterRegionStep::WriteDense {
                    pc,
                    index: index_location.lane(),
                    source: source_location.lane(),
                }
            }
            RegionOp::ReadStatic { dst, .. } if numeric.contains(dst) => {
                let destination = allocate_destination(
                    pc,
                    *dst,
                    RegisterLocation::F64(0),
                    &locations,
                    &mut maximum_live_word_lanes,
                    &mut maximum_live_f64_lanes,
                )?
                .lane();
                locations.insert(*dst, RegisterLocation::F64(destination));
                RegisterRegionStep::LoadStatic { pc, destination }
            }
            RegionOp::ReadStatic { .. } => RegisterRegionStep::CopyReadStatic { pc },
            RegionOp::WriteStatic { src, .. } if numeric.contains(src) => {
                let source = prepare_f64(
                    pc,
                    *src,
                    &locations,
                    &mut temporaries,
                    &mut steps,
                    &mut conversions,
                )?;
                consume(pc, *src, &mut remaining, &mut locations)?;
                RegisterRegionStep::WriteStatic {
                    pc,
                    source: source.lane(),
                }
            }
            RegionOp::WriteStatic { .. } => RegisterRegionStep::CopyWriteStatic { pc },
            RegionOp::Jump { target, .. } => {
                if !locations.is_empty() {
                    return Err(RegisterRegionReject::LiveValuesAtBackedge {
                        pc,
                        count: locations.len(),
                    });
                }
                RegisterRegionStep::Jump {
                    pc,
                    target: *target,
                }
            }
            RegionOp::JumpIfFalse { .. } => {
                return Err(RegisterRegionReject::UnsupportedBooleanUse { pc });
            }
        };
        steps.push(step);
        index += 1;
    }

    if numeric_operations < MIN_REGISTER_REGION_NUMERIC_OPERATIONS {
        return Err(RegisterRegionReject::TooLittleNumericWork);
    }
    if operations.iter().any(|operation| match operation {
        RegionOp::Jump { target, .. } | RegionOp::JumpIfFalse { target, .. } => {
            !(quote.start..quote.end).contains(target) && *target != quote.exit
        }
        _ => false,
    }) {
        return Err(RegisterRegionReject::NotSingleTrace);
    }
    Ok(RegisterRegionPlan {
        start: quote.start,
        end: quote.end,
        exit: quote.exit,
        steps: steps.into_boxed_slice(),
        maximum_live_word_lanes,
        maximum_live_f64_lanes,
        numeric_operations,
        conversions,
        forwarded_local_loads,
        alias_updates,
        maximum_location_fanout,
    })
}

fn word32_result(kind: NumericBinary) -> Option<Word32Kind> {
    match kind {
        NumericBinary::BitOr
        | NumericBinary::BitXor
        | NumericBinary::BitAnd
        | NumericBinary::ShiftLeft
        | NumericBinary::ShiftRight => Some(Word32Kind::Signed),
        NumericBinary::ShiftRightUnsigned => Some(Word32Kind::Unsigned),
        NumericBinary::Add
        | NumericBinary::Subtract
        | NumericBinary::Multiply
        | NumericBinary::Divide
        | NumericBinary::Equal
        | NumericBinary::NotEqual
        | NumericBinary::StrictEqual
        | NumericBinary::StrictNotEqual
        | NumericBinary::Less
        | NumericBinary::LessEqual
        | NumericBinary::Greater
        | NumericBinary::GreaterEqual => None,
    }
}

fn value_has_only_word32_uses(register: Register, operations: &[RegionOp]) -> bool {
    let mut saw_use = false;
    for operation in operations {
        let compatible = match operation {
            RegionOp::Unary { src, kind, .. } if *src == register => {
                saw_use = true;
                *kind == NumericUnary::BitNot
            }
            RegionOp::Binary {
                left, right, kind, ..
            } if *left == register || *right == register => {
                saw_use = true;
                word32_result(*kind).is_some()
            }
            RegionOp::Move { src, .. } if *src == register => {
                saw_use = true;
                true
            }
            RegionOp::WriteLocal { src, .. }
            | RegionOp::WriteDense { src, .. }
            | RegionOp::WriteStatic { src, .. }
                if *src == register =>
            {
                saw_use = true;
                false
            }
            RegionOp::ReadDense { index, .. } if *index == register => {
                saw_use = true;
                false
            }
            RegionOp::JumpIfFalse { test, .. } if *test == register => {
                saw_use = true;
                false
            }
            _ => true,
        };
        if !compatible {
            return false;
        }
    }
    saw_use
}

fn prepare_f64(
    pc: usize,
    register: Register,
    locations: &BTreeMap<Register, RegisterLocation>,
    temporaries: &mut TemporaryLanes,
    steps: &mut Vec<RegisterRegionStep>,
    conversions: &mut usize,
) -> Result<RegisterLocation, RegisterRegionReject> {
    let source_location = location(pc, register, locations)?;
    if let Some(converted) = temporaries.converted.get(&source_location).copied()
        && matches!(converted, RegisterLocation::F64(_))
    {
        return Ok(converted);
    }
    let RegisterLocation::Word32 {
        lane: source,
        kind: source_kind,
    } = source_location
    else {
        return Ok(source_location);
    };
    let destination = allocate_temporary_f64(pc, locations, temporaries)?;
    let kind = match source_kind {
        Word32Kind::Signed => RegisterConversion::SignedWord32ToF64,
        Word32Kind::Unsigned => RegisterConversion::UnsignedWord32ToF64,
    };
    steps.push(RegisterRegionStep::Convert {
        pc,
        destination,
        source,
        kind,
    });
    *conversions += 1;
    let converted = RegisterLocation::F64(destination);
    temporaries.converted.insert(source_location, converted);
    Ok(converted)
}

fn prepare_word32(
    pc: usize,
    register: Register,
    locations: &BTreeMap<Register, RegisterLocation>,
    temporaries: &mut TemporaryLanes,
    steps: &mut Vec<RegisterRegionStep>,
    conversions: &mut usize,
) -> Result<RegisterLocation, RegisterRegionReject> {
    let source_location = location(pc, register, locations)?;
    if let Some(converted) = temporaries.converted.get(&source_location).copied()
        && matches!(converted, RegisterLocation::Word32 { .. })
    {
        return Ok(converted);
    }
    let RegisterLocation::F64(source) = source_location else {
        return Ok(source_location);
    };
    let destination = allocate_temporary_word(pc, locations, temporaries)?;
    steps.push(RegisterRegionStep::Convert {
        pc,
        destination,
        source,
        kind: RegisterConversion::F64ToWord32,
    });
    *conversions += 1;
    let converted = RegisterLocation::Word32 {
        lane: destination,
        kind: Word32Kind::Signed,
    };
    temporaries.converted.insert(source_location, converted);
    Ok(converted)
}

fn location_fanout(
    location: RegisterLocation,
    locations: &BTreeMap<Register, RegisterLocation>,
) -> usize {
    locations
        .values()
        .filter(|candidate| **candidate == location)
        .count()
}

fn allocate_destination(
    pc: usize,
    destination: Register,
    shape: RegisterLocation,
    locations: &BTreeMap<Register, RegisterLocation>,
    maximum_live_word_lanes: &mut usize,
    maximum_live_f64_lanes: &mut usize,
) -> Result<RegisterLocation, RegisterRegionReject> {
    if locations.contains_key(&destination) {
        return Err(register_pressure(pc, locations));
    }
    match shape {
        RegisterLocation::Word32 { kind, .. } => {
            let lane = free_word_lane(locations, &BTreeSet::new())
                .ok_or_else(|| register_pressure(pc, locations))?;
            *maximum_live_word_lanes =
                (*maximum_live_word_lanes).max(live_word_lanes(locations) + 1);
            Ok(RegisterLocation::Word32 { lane, kind })
        }
        RegisterLocation::F64(_) => {
            let lane = free_f64_lane(locations, &BTreeSet::new())
                .ok_or_else(|| register_pressure(pc, locations))?;
            *maximum_live_f64_lanes = (*maximum_live_f64_lanes).max(live_f64_lanes(locations) + 1);
            Ok(RegisterLocation::F64(lane))
        }
    }
}

fn allocate_temporary_word(
    pc: usize,
    locations: &BTreeMap<Register, RegisterLocation>,
    temporaries: &mut TemporaryLanes,
) -> Result<RegisterLane, RegisterRegionReject> {
    let lane = free_word_lane(locations, &temporaries.words)
        .ok_or_else(|| register_pressure(pc, locations))?;
    temporaries.words.insert(lane);
    Ok(lane)
}

fn allocate_temporary_f64(
    pc: usize,
    locations: &BTreeMap<Register, RegisterLocation>,
    temporaries: &mut TemporaryLanes,
) -> Result<RegisterLane, RegisterRegionReject> {
    let lane = free_f64_lane(locations, &temporaries.numbers)
        .ok_or_else(|| register_pressure(pc, locations))?;
    temporaries.numbers.insert(lane);
    Ok(lane)
}

fn free_word_lane(
    locations: &BTreeMap<Register, RegisterLocation>,
    reserved: &BTreeSet<RegisterLane>,
) -> Option<RegisterLane> {
    let used = locations
        .values()
        .filter_map(|location| match location {
            RegisterLocation::Word32 { lane, .. } => Some(*lane),
            RegisterLocation::F64(_) => None,
        })
        .chain(reserved.iter().copied())
        .collect::<BTreeSet<_>>();
    (0..REGISTER_REGION_WORD_LANE_COUNT)
        .map(|lane| lane as RegisterLane)
        .find(|lane| !used.contains(lane))
}

fn free_f64_lane(
    locations: &BTreeMap<Register, RegisterLocation>,
    reserved: &BTreeSet<RegisterLane>,
) -> Option<RegisterLane> {
    let used = locations
        .values()
        .filter_map(|location| match location {
            RegisterLocation::F64(lane) => Some(*lane),
            RegisterLocation::Word32 { .. } => None,
        })
        .chain(reserved.iter().copied())
        .collect::<BTreeSet<_>>();
    (0..REGISTER_REGION_F64_LANE_COUNT)
        .map(|lane| lane as RegisterLane)
        .find(|lane| !used.contains(lane))
}

fn live_word_lanes(locations: &BTreeMap<Register, RegisterLocation>) -> usize {
    locations
        .values()
        .filter_map(|location| match location {
            RegisterLocation::Word32 { lane, .. } => Some(*lane),
            RegisterLocation::F64(_) => None,
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn live_f64_lanes(locations: &BTreeMap<Register, RegisterLocation>) -> usize {
    locations
        .values()
        .filter_map(|location| match location {
            RegisterLocation::F64(lane) => Some(*lane),
            RegisterLocation::Word32 { .. } => None,
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn register_pressure(
    pc: usize,
    locations: &BTreeMap<Register, RegisterLocation>,
) -> RegisterRegionReject {
    RegisterRegionReject::RegisterPressure {
        pc,
        word_lanes: live_word_lanes(locations) + 1,
        f64_lanes: live_f64_lanes(locations) + 1,
    }
}

fn location(
    pc: usize,
    register: Register,
    locations: &BTreeMap<Register, RegisterLocation>,
) -> Result<RegisterLocation, RegisterRegionReject> {
    locations
        .get(&register)
        .copied()
        .ok_or(RegisterRegionReject::MissingNumericValue { pc, register })
}

fn consume(
    pc: usize,
    register: Register,
    remaining: &mut BTreeMap<Register, usize>,
    locations: &mut BTreeMap<Register, RegisterLocation>,
) -> Result<RegisterLocation, RegisterRegionReject> {
    let location = location(pc, register, locations)?;
    let count = remaining
        .get_mut(&register)
        .ok_or(RegisterRegionReject::MissingNumericValue { pc, register })?;
    *count = count
        .checked_sub(1)
        .ok_or(RegisterRegionReject::MissingNumericValue { pc, register })?;
    if *count == 0 {
        locations.remove(&register);
    }
    Ok(location)
}

fn numeric_registers(quote: &QuotedLoop, operations: &[RegionOp]) -> BTreeSet<Register> {
    let mut numeric = BTreeSet::new();
    for operation in operations {
        if let RegionOp::NumberLiteral { dst, .. }
        | RegionOp::Unary { dst, .. }
        | RegionOp::ReadDense { dst, .. } = operation
        {
            numeric.insert(*dst);
        }
        if let RegionOp::Binary { dst, kind, .. } = operation
            && !kind.result_is_boolean()
        {
            numeric.insert(*dst);
        }
        match operation {
            RegionOp::Unary { src, .. } => {
                numeric.insert(*src);
            }
            RegionOp::Binary { left, right, .. } => {
                numeric.insert(*left);
                numeric.insert(*right);
            }
            RegionOp::ReadDense { index, .. } => {
                numeric.insert(*index);
            }
            RegionOp::WriteDense { index, src, .. } => {
                numeric.insert(*index);
                numeric.insert(*src);
            }
            RegionOp::Elided { .. }
            | RegionOp::NumberLiteral { .. }
            | RegionOp::ReadLocal { .. }
            | RegionOp::ReadCaptured { .. }
            | RegionOp::WriteLocal { .. }
            | RegionOp::Move { .. }
            | RegionOp::ReadStatic { .. }
            | RegionOp::WriteStatic { .. }
            | RegionOp::Jump { .. }
            | RegionOp::JumpIfFalse { .. } => {}
        }
    }
    for requirement in quote.property_requirements() {
        if requirement.value_kind == PropertyValueKind::Number
            && let Some(RegionOp::ReadStatic { dst, .. }) = operations
                .iter()
                .find(|operation| operation_pc(operation) == requirement.pc)
        {
            numeric.insert(*dst);
        }
    }
    loop {
        let before = numeric.len();
        for operation in operations {
            if let RegionOp::Move { dst, src, .. } = operation
                && (numeric.contains(dst) || numeric.contains(src))
            {
                numeric.insert(*dst);
                numeric.insert(*src);
            }
        }
        if numeric.len() == before {
            break;
        }
    }
    numeric
}

fn numeric_use_counts(
    operations: &[RegionOp],
    numeric: &BTreeSet<Register>,
    boolean_results: &BTreeSet<Register>,
    internal_local_sources: &BTreeMap<Register, Register>,
) -> BTreeMap<Register, usize> {
    let mut counts = BTreeMap::new();
    for operation in operations {
        let sources = match operation {
            RegionOp::WriteLocal { src, .. }
            | RegionOp::Unary { src, .. }
            | RegionOp::Move { src, .. }
            | RegionOp::WriteStatic { src, .. } => vec![*src],
            RegionOp::Binary { left, right, .. } => vec![*left, *right],
            RegionOp::ReadDense { index, .. } => vec![*index],
            RegionOp::WriteDense { index, src, .. } => vec![*index, *src],
            RegionOp::JumpIfFalse { test, .. } if !boolean_results.contains(test) => vec![*test],
            _ => Vec::new(),
        };
        for source in sources {
            if numeric.contains(&source) {
                *counts.entry(source).or_insert(0) += 1;
            }
        }
    }
    for operation in operations {
        if let RegionOp::ReadLocal { dst, .. } = operation
            && numeric.contains(dst)
            && let Some(source) = internal_local_sources.get(dst)
        {
            *counts.entry(*source).or_insert(0) += 1;
        }
    }
    counts
}

fn operation_pc(operation: &RegionOp) -> usize {
    match operation {
        RegionOp::Elided { pc }
        | RegionOp::NumberLiteral { pc, .. }
        | RegionOp::ReadLocal { pc, .. }
        | RegionOp::ReadCaptured { pc, .. }
        | RegionOp::WriteLocal { pc, .. }
        | RegionOp::Move { pc, .. }
        | RegionOp::Unary { pc, .. }
        | RegionOp::Binary { pc, .. }
        | RegionOp::ReadDense { pc, .. }
        | RegionOp::WriteDense { pc, .. }
        | RegionOp::ReadStatic { pc, .. }
        | RegionOp::WriteStatic { pc, .. }
        | RegionOp::Jump { pc, .. }
        | RegionOp::JumpIfFalse { pc, .. } => *pc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric_region::model::{NumericDenseState, Region, RegionNode, RewriteStats};

    const LOOP_HEADER_PC: usize = 0;
    const BLOCK_START_PC: usize = 0;
    const LOCAL_SLOT: usize = 0;
    const WORD_OPERATION_COUNT: usize = 8;
    const FIRST_INPUT_REGISTER: Register = 0;
    const FIRST_LITERAL_REGISTER: Register = 16;
    const FIRST_RESULT_REGISTER: Register = 32;
    const FIRST_ALIAS_REGISTER: Register = 48;
    const SHARED_SOURCE_AND_ALIAS_FANOUT: usize = 2;
    const FIRST_WORD_LITERAL: f64 = 1.0;
    const NEXT_BYTECODE_PC_DISTANCE: usize = 1;

    #[test]
    fn physical_cover_keeps_word32_values_and_forwards_local_identity() {
        let mut operations = vec![RegionOp::ReadLocal {
            pc: LOOP_HEADER_PC,
            dst: FIRST_INPUT_REGISTER,
            slot: LOCAL_SLOT,
        }];
        let mut internal_local_sources = BTreeMap::new();
        let mut input = FIRST_INPUT_REGISTER;
        let mut pc = operations.len();
        for operation_index in 0..WORD_OPERATION_COUNT {
            let literal = FIRST_LITERAL_REGISTER + operation_index as Register;
            let result = FIRST_RESULT_REGISTER + operation_index as Register;
            operations.push(RegionOp::NumberLiteral {
                pc,
                dst: literal,
                bits: (operation_index as f64 + FIRST_WORD_LITERAL).to_bits(),
            });
            pc += 1;
            operations.push(RegionOp::Binary {
                pc,
                dst: result,
                left: input,
                right: literal,
                kind: NumericBinary::BitXor,
            });
            pc += 1;
            operations.push(RegionOp::WriteLocal {
                pc,
                slot: LOCAL_SLOT,
                src: result,
            });
            pc += 1;
            if operation_index + 1 < WORD_OPERATION_COUNT {
                let next_input = FIRST_INPUT_REGISTER + operation_index as Register + 1;
                operations.push(RegionOp::ReadLocal {
                    pc,
                    dst: next_input,
                    slot: LOCAL_SLOT,
                });
                internal_local_sources.insert(next_input, result);
                input = next_input;
                pc += 1;
            }
        }
        operations.push(RegionOp::Jump {
            pc,
            target: LOOP_HEADER_PC,
        });
        let exit = pc + NEXT_BYTECODE_PC_DISTANCE;
        let body = RegionNode::Seq(operations.iter().cloned().map(RegionNode::Op).collect());
        let quote = QuotedLoop {
            start: LOOP_HEADER_PC,
            end: exit,
            exit,
            region: Region::<NumericDenseState, NumericDenseState>::new(RegionNode::Trace {
                header: LOOP_HEADER_PC,
                exit,
                body: Box::new(body),
            }),
            requirements: Vec::new(),
            internal_local_sources,
            internal_number_loads: Vec::new(),
            proven_index_sites: Vec::new(),
            property_requirements: Vec::new(),
            rewrite_stats: RewriteStats::default(),
        };

        let plan = plan_register_region(&quote).expect("mixed register cover");
        assert_eq!(plan.numeric_operations, WORD_OPERATION_COUNT);
        assert_eq!(plan.forwarded_local_loads, WORD_OPERATION_COUNT - 1);
        assert!(
            plan.steps
                .iter()
                .any(|step| matches!(step, RegisterRegionStep::LoadWordLocal { .. }))
        );
        assert_eq!(
            plan.steps
                .iter()
                .filter(|step| matches!(step, RegisterRegionStep::LoadWordLiteral { .. }))
                .count(),
            WORD_OPERATION_COUNT,
        );
        assert!(plan.steps.iter().any(|step| matches!(
            step,
            RegisterRegionStep::Binary {
                destination: RegisterLocation::Word32 { .. },
                ..
            }
        )));
    }

    #[test]
    fn physical_cover_represents_moves_as_shared_location_aliases() {
        let mut operations = vec![RegionOp::ReadLocal {
            pc: LOOP_HEADER_PC,
            dst: FIRST_INPUT_REGISTER,
            slot: LOCAL_SLOT,
        }];
        let mut input = FIRST_INPUT_REGISTER;
        let mut pc = operations.len();
        for operation_index in 0..WORD_OPERATION_COUNT {
            let alias = FIRST_ALIAS_REGISTER + operation_index as Register;
            let result = FIRST_RESULT_REGISTER + operation_index as Register;
            operations.push(RegionOp::Move {
                pc,
                dst: alias,
                src: input,
            });
            pc += 1;
            operations.push(RegionOp::Binary {
                pc,
                dst: result,
                left: input,
                right: alias,
                kind: NumericBinary::BitXor,
            });
            pc += 1;
            input = result;
        }
        operations.push(RegionOp::WriteLocal {
            pc,
            slot: LOCAL_SLOT,
            src: input,
        });
        pc += 1;
        operations.push(RegionOp::Jump {
            pc,
            target: LOOP_HEADER_PC,
        });
        let exit = pc + NEXT_BYTECODE_PC_DISTANCE;
        let body = RegionNode::Seq(operations.iter().cloned().map(RegionNode::Op).collect());
        let quote = QuotedLoop {
            start: LOOP_HEADER_PC,
            end: exit,
            exit,
            region: Region::<NumericDenseState, NumericDenseState>::new(RegionNode::Trace {
                header: LOOP_HEADER_PC,
                exit,
                body: Box::new(body),
            }),
            requirements: Vec::new(),
            internal_local_sources: BTreeMap::new(),
            internal_number_loads: Vec::new(),
            proven_index_sites: Vec::new(),
            property_requirements: Vec::new(),
            rewrite_stats: RewriteStats::default(),
        };

        let plan = plan_register_region(&quote).expect("alias-preserving register cover");
        assert_eq!(plan.alias_updates, WORD_OPERATION_COUNT);
        assert_eq!(plan.maximum_location_fanout, SHARED_SOURCE_AND_ALIAS_FANOUT);
        assert!(
            !plan
                .steps
                .iter()
                .any(|step| matches!(step, RegisterRegionStep::Move { .. }))
        );
    }

    #[test]
    fn physical_cover_accepts_a_straight_line_numeric_block() {
        let mut operations = vec![RegionOp::ReadLocal {
            pc: BLOCK_START_PC,
            dst: FIRST_INPUT_REGISTER,
            slot: LOCAL_SLOT,
        }];
        let mut input = FIRST_INPUT_REGISTER;
        let mut pc = operations.len();
        for operation_index in 0..WORD_OPERATION_COUNT {
            let literal = FIRST_LITERAL_REGISTER + operation_index as Register;
            let result = FIRST_RESULT_REGISTER + operation_index as Register;
            operations.push(RegionOp::NumberLiteral {
                pc,
                dst: literal,
                bits: (operation_index as f64 + FIRST_WORD_LITERAL).to_bits(),
            });
            pc += 1;
            operations.push(RegionOp::Binary {
                pc,
                dst: result,
                left: input,
                right: literal,
                kind: NumericBinary::BitXor,
            });
            pc += 1;
            input = result;
        }
        operations.push(RegionOp::WriteLocal {
            pc,
            slot: LOCAL_SLOT,
            src: input,
        });
        let block_end = pc + NEXT_BYTECODE_PC_DISTANCE;
        let quote = QuotedLoop {
            start: BLOCK_START_PC,
            end: block_end,
            exit: block_end,
            region: Region::<NumericDenseState, NumericDenseState>::new(RegionNode::Seq(
                operations.into_iter().map(RegionNode::Op).collect(),
            )),
            requirements: Vec::new(),
            internal_local_sources: BTreeMap::new(),
            internal_number_loads: Vec::new(),
            proven_index_sites: Vec::new(),
            property_requirements: Vec::new(),
            rewrite_stats: RewriteStats::default(),
        };

        let plan = plan_register_region(&quote).expect("straight-line register cover");
        assert_eq!(plan.numeric_operations, WORD_OPERATION_COUNT);
        assert_eq!(plan.start, BLOCK_START_PC);
        assert_eq!(plan.exit, block_end);
    }
}
