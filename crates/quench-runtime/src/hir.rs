//! Common typed register program. NativeIR, FastIR, and DynamicIR are subsets.
//!
//! One instruction enum. A layer is derived from the op: NIR is proven Native
//! ops, FIR is guarded Fast ops, DIR is Dynamic ops. Guard and box are the
//! only crossings. There is not a second IR per layer.

use crate::layer::{GuardKind, Layer};

pub use crate::hir_gc::{CatchClause, GcOp, GcStorage, GcType};
use crate::native::{BinF32, BinF64, BinI32, BinI64, ConvOp, SimdOp, UnF32, UnF64, UnI32, UnI64};

pub type Reg = u16;

/// Value kind independent of layer. Layer lives beside it, not inside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Kind {
    I32,
    I64,
    F32,
    F64,
    V128,
    Ref,
    Dynamic,
}

/// Register type: one layer, one kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Ty {
    pub layer: Layer,
    pub kind: Kind,
}

impl Ty {
    pub const NATIVE_I32: Self = Self {
        layer: Layer::Native,
        kind: Kind::I32,
    };

    pub const NATIVE_I64: Self = Self {
        layer: Layer::Native,
        kind: Kind::I64,
    };

    pub const DYNAMIC: Self = Self {
        layer: Layer::Dynamic,
        kind: Kind::Dynamic,
    };

    pub fn native(kind: Kind) -> Self {
        Self {
            layer: Layer::Native,
            kind,
        }
    }
}

/// One instruction. Native, Fast, and Dynamic ops share this enum.
#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    Nop,
    Unreachable,
    ConstI32 {
        dst: Reg,
        val: i32,
    },
    ConstI64 {
        dst: Reg,
        val: i64,
    },
    ConstF32 {
        dst: Reg,
        bits: u32,
    },
    ConstF64 {
        dst: Reg,
        bits: u64,
    },
    ConstV128 {
        dst: Reg,
        bits: u128,
    },
    ConstRefNull {
        dst: Reg,
    },
    ConstRefFunc {
        dst: Reg,
        func: u32,
    },
    UnI32 {
        op: UnI32,
        dst: Reg,
        src: Reg,
    },
    BinI32 {
        op: BinI32,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    UnI64 {
        op: UnI64,
        dst: Reg,
        src: Reg,
    },
    BinI64 {
        op: BinI64,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    UnF32 {
        op: UnF32,
        dst: Reg,
        src: Reg,
    },
    BinF32 {
        op: BinF32,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    UnF64 {
        op: UnF64,
        dst: Reg,
        src: Reg,
    },
    BinF64 {
        op: BinF64,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Conv {
        op: ConvOp,
        dst: Reg,
        src: Reg,
    },
    Move {
        dst: Reg,
        src: Reg,
    },
    Select {
        dst: Reg,
        a: Reg,
        b: Reg,
        cond: Reg,
    },
    Jump {
        target: u32,
    },
    JumpIf {
        cond: Reg,
        target: u32,
        zero: bool,
    },
    JumpTable {
        index: Reg,
        targets: Box<[u32]>,
        default: u32,
    },
    Call {
        func: u32,
        args: Box<[Reg]>,
        dsts: Box<[Reg]>,
    },
    CallIndirect {
        table: u32,
        type_idx: u32,
        index: Reg,
        args: Box<[Reg]>,
        dsts: Box<[Reg]>,
    },
    ReturnCall {
        func: u32,
        args: Box<[Reg]>,
    },
    ReturnCallIndirect {
        table: u32,
        type_idx: u32,
        index: Reg,
        args: Box<[Reg]>,
    },
    CallRef {
        type_idx: u32,
        func: Reg,
        args: Box<[Reg]>,
        dsts: Box<[Reg]>,
    },
    SimdShuffle {
        dst: Reg,
        a: Reg,
        b: Reg,
        lanes: [u8; 16],
    },
    Wide {
        op: WideOp,
        dst_lo: Reg,
        dst_hi: Reg,
        a: Reg,
        b: Reg,
        c: Reg,
        d: Reg,
    },
    Return {
        srcs: Box<[Reg]>,
    },
    Load {
        dst: Reg,
        addr: Reg,
        offset: u64,
        mem: u32,
        op: LoadOp,
    },
    Store {
        addr: Reg,
        src: Reg,
        offset: u64,
        mem: u32,
        op: StoreOp,
    },
    MemorySize {
        dst: Reg,
        mem: u32,
    },
    MemoryGrow {
        dst: Reg,
        delta: Reg,
        mem: u32,
    },
    MemoryCopy {
        dst_mem: u32,
        src_mem: u32,
        dst: Reg,
        src: Reg,
        len: Reg,
    },
    MemoryFill {
        mem: u32,
        dst: Reg,
        val: Reg,
        len: Reg,
    },
    MemoryInit {
        mem: u32,
        data: u32,
        dst: Reg,
        src: Reg,
        len: Reg,
    },
    DataDrop {
        data: u32,
    },
    GlobalGet {
        dst: Reg,
        global: u32,
    },
    GlobalSet {
        global: u32,
        src: Reg,
    },
    TableGet {
        dst: Reg,
        table: u32,
        index: Reg,
    },
    TableSet {
        table: u32,
        index: Reg,
        src: Reg,
    },
    TableSize {
        dst: Reg,
        table: u32,
    },
    TableGrow {
        dst: Reg,
        table: u32,
        fill: Reg,
        delta: Reg,
    },
    TableFill {
        table: u32,
        dst: Reg,
        val: Reg,
        len: Reg,
    },
    TableCopy {
        dst_table: u32,
        src_table: u32,
        dst: Reg,
        src: Reg,
        len: Reg,
    },
    TableInit {
        table: u32,
        elem: u32,
        dst: Reg,
        src: Reg,
        len: Reg,
    },
    ElemDrop {
        elem: u32,
    },
    RefIsNull {
        dst: Reg,
        src: Reg,
    },
    RefAsNonNull {
        src: Reg,
    },
    RefEq {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    RefI31 {
        dst: Reg,
        src: Reg,
    },
    I31Get {
        dst: Reg,
        src: Reg,
        signed: bool,
    },
    Simd {
        op: SimdOp,
        dst: Reg,
        a: Reg,
        b: Reg,
        c: Reg,
        lane: u8,
    },
    BoxToDynamic {
        dst: Reg,
        src: Reg,
    },
    Guard {
        dst: Reg,
        src: Reg,
        kind: GuardKind,
    },
    Gc {
        op: GcOp,
        dst: Reg,
        args: Box<[Reg]>,
    },
    Throw {
        tag: u32,
        args: Box<[Reg]>,
    },
    ThrowRef {
        src: Reg,
    },
    ReturnCallRef {
        type_idx: u32,
        func: Reg,
        args: Box<[Reg]>,
    },
    TryBegin {
        catches: Box<[CatchClause]>,
    },
    TryEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WideOp {
    Add128,
    Sub128,
    MulWideS,
    MulWideU,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadOp {
    I32,
    I64,
    F32,
    F64,
    I32_8S,
    I32_8U,
    I32_16S,
    I32_16U,
    I64_8S,
    I64_8U,
    I64_16S,
    I64_16U,
    I64_32S,
    I64_32U,
    V128,
    V128Splat8,
    V128Splat16,
    V128Splat32,
    V128Splat64,
    V128Zero32,
    V128Zero64,
    V128Ext8x8S,
    V128Ext8x8U,
    V128Ext16x4S,
    V128Ext16x4U,
    V128Ext32x2S,
    V128Ext32x2U,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOp {
    I32,
    I64,
    F32,
    F64,
    I32_8,
    I32_16,
    I64_8,
    I64_16,
    I64_32,
    V128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirFunc {
    pub params: Box<[Ty]>,
    pub results: Box<[Ty]>,
    pub locals: Box<[Ty]>,
    pub nregs: u16,
    pub code: Box<[Inst]>,
}

/// NativeIR: HIR restricted to Native ops. Not a second instruction set.
pub type NativeIR = HirFunc;
/// FastIR: HIR restricted to Fast (guarded) ops.
pub type FastIR = HirFunc;
/// DynamicIR: HIR with Dynamic ops; meaning resolved at run time.
pub type DynamicIR = HirFunc;

pub type Nir = NativeIR;
pub type Fir = FastIR;
pub type Dir = DynamicIR;

impl Inst {
    /// Which subset this op belongs to. Guard/box are the only crossings.
    pub fn ir(&self) -> Layer {
        match self {
            Self::BoxToDynamic { .. } => Layer::Dynamic,
            Self::Guard { .. } => Layer::Fast,
            _ => Layer::Native,
        }
    }
}

impl HirFunc {
    pub fn ir(&self) -> Layer {
        self.code
            .iter()
            .map(Inst::ir)
            .fold(Layer::Native, Layer::join)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Export {
    Func(u32),
    Table(u32),
    Memory(u32),
    Global(u32),
    Tag(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncSig {
    pub params: Box<[Kind]>,
    pub results: Box<[Kind]>,
    pub rec_len: u32,
    pub rec_index: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum HeapKind {
    Func,
    Extern,
    Concrete,
    NoFunc,
    NoExtern,
    Any,
    Eq,
    I31,
    Struct,
    Array,
    None,
    Exn,
    NoExn,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefType {
    pub heap: HeapKind,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirGlobal {
    pub ty: Ty,
    pub refty: Option<RefType>,
    pub mutable: bool,
    pub init: ConstExpr,
}

/// One const-expression operator. Evaluated as a stack at instantiate time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstOp {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    V128(u128),
    RefNull,
    RefFunc(u32),
    GlobalGet(u32),
    I32Add,
    I32Sub,
    I32Mul,
    I32And,
    I32Or,
    I32Xor,
    I64Add,
    I64Sub,
    I64Mul,
    I64And,
    I64Or,
    I64Xor,
    ArrayNewDefault(u32),
    ArrayNew(u32),
    StructNewDefault(u32),
    StructNew(u32),
    ArrayNewFixed { type_idx: u32, n: u32 },
    RefI31,
    AnyConvertExtern,
    ExternConvertAny,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstExpr {
    pub ops: Box<[ConstOp]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirMemory {
    pub memory64: bool,
    pub shared: bool,
    pub initial: u64,
    pub maximum: Option<u64>,
    pub page_size_log2: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirTable {
    pub table64: bool,
    pub initial: u64,
    pub maximum: Option<u64>,
    pub refty: RefType,
    pub init: Option<ConstExpr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirData {
    pub mem: u32,
    pub offset: Option<ConstExpr>,
    pub bytes: Box<[u8]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirElem {
    pub table: u32,
    pub offset: Option<ConstExpr>,
    pub declared: bool,
    pub items: Box<[ConstExpr]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirImport {
    pub module: Box<str>,
    pub name: Box<str>,
    pub kind: ImportKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportKind {
    Func {
        type_idx: u32,
    },
    Table(HirTable),
    Memory(HirMemory),
    Global {
        ty: Ty,
        refty: Option<RefType>,
        mutable: bool,
    },
    Tag {
        type_idx: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirModule {
    pub types: Box<[FuncSig]>,
    /// Defined functions only (not imports). `None` = not yet lowered.
    pub funcs: Box<[Option<HirFunc>]>,
    /// Type index for every function, imports first.
    pub func_types: Box<[u32]>,
    pub imports: Box<[HirImport]>,
    pub exports: Box<[(Box<str>, Export)]>,
    pub memories: Box<[HirMemory]>,
    pub tables: Box<[HirTable]>,
    pub globals: Box<[HirGlobal]>,
    pub datas: Box<[HirData]>,
    pub elems: Box<[HirElem]>,
    pub tags: Box<[u32]>,
    pub gc_types: Box<[GcType]>,
    pub start: Option<u32>,
}
