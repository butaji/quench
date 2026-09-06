#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegionDeclaration {
    pub(crate) name: &'static str,
    pub(crate) operations: &'static [&'static str],
    pub(crate) abi: DeclAbi,
    pub(crate) x86_bytes: &'static [u8],
    pub(crate) aarch64_bytes: &'static [u8],
    pub(crate) portable_bytes: &'static [u8],
    pub(crate) holes: &'static [(u16, usize, &'static str)],
    pub(crate) aarch64_holes: &'static [(u16, usize, &'static str)],
    pub(crate) entry: u32,
    pub(crate) external_entries: &'static [u32],
}

pub(crate) fn region_key_name(name: &str) -> String {
    match name {
        "set_named" => "SET_N".to_owned(),
        other => other.to_ascii_uppercase(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeclAbi {
    ScalarF64Binary,
    ScalarF64Unary,
    ScalarF64x3,
    TaggedWord,
    ConstantWord,
    ScalarBool,
    ScalarWordBool,
    ScalarWordPairBool,
    ScalarI32,
    ScalarU32,
    Bridge,
    ArrayKernel,
    ArrayNumericLoop,
    CompareBranch,
    PropertyGuard,
    PropertyWriteGuard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecipeComposition {
    Whole,
    LinkedFragments,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalOperandField {
    A,
    B,
    C,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalOperand {
    pub(crate) operation: u8,
    pub(crate) field: PhysicalOperandField,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalBindingValue {
    Operand(PhysicalOperand),
    RegionStart,
    RegionEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalBinding {
    Equal(PhysicalBindingValue, PhysicalBindingValue),
    AllDistinct(&'static [PhysicalOperand]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalOutputValue {
    Array,
    Element,
    Index,
    Result,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalOutputDestination {
    Register(PhysicalOperand),
    LocalSlot(PhysicalOperand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalOutput {
    pub(crate) value: PhysicalOutputValue,
    pub(crate) destination: PhysicalOutputDestination,
}

impl PhysicalBinding {
    pub(crate) const fn valid_for(self, operation_count: usize) -> bool {
        match self {
            Self::Equal(left, right) => {
                left.valid_for(operation_count) && right.valid_for(operation_count)
            }
            Self::AllDistinct(operands) => {
                if operands.len() < 2 {
                    return false;
                }
                let mut index = 0;
                while index < operands.len() {
                    if operands[index].operation as usize >= operation_count {
                        return false;
                    }
                    index += 1;
                }
                true
            }
        }
    }
}

impl PhysicalBindingValue {
    const fn valid_for(self, operation_count: usize) -> bool {
        match self {
            Self::Operand(operand) => (operand.operation as usize) < operation_count,
            Self::RegionStart | Self::RegionEnd => true,
        }
    }
}

impl PhysicalOutput {
    pub(crate) const fn valid_for(self, operation_count: usize) -> bool {
        let operand = match self.destination {
            PhysicalOutputDestination::Register(operand)
            | PhysicalOutputDestination::LocalSlot(operand) => operand,
        };
        (operand.operation as usize) < operation_count
    }
}

pub(crate) const fn operand(operation: u8, field: PhysicalOperandField) -> PhysicalOperand {
    PhysicalOperand { operation, field }
}

pub(crate) const fn value(operation: u8, field: PhysicalOperandField) -> PhysicalBindingValue {
    PhysicalBindingValue::Operand(operand(operation, field))
}

pub(crate) const fn equal(
    left: PhysicalBindingValue,
    right: PhysicalBindingValue,
) -> PhysicalBinding {
    PhysicalBinding::Equal(left, right)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AssemblyContinuation {
    pub(crate) head_name: &'static str,
    pub(crate) tail_name: &'static str,
    pub(crate) target: &'static str,
}

macro_rules! assembly_continuation {
    () => {
        None
    };
    (($head:literal, $tail:literal, $target:literal)) => {
        Some(AssemblyContinuation {
            head_name: $head,
            tail_name: $tail,
            target: $target,
        })
    };
}

macro_rules! recipe_composition {
    () => {
        RecipeComposition::Whole
    };
    ($composition:ident) => {
        RecipeComposition::$composition
    };
}

macro_rules! rust_leaf_catalog {
    ($( $variant:ident {
        name: $name:literal, abi: $abi:ident, ops: [$($op:literal),+],
        params: $params:literal, result: $result:literal, body: $body:literal,
        x86: $x86:expr, aarch64: $aarch64:expr,
        holes: $holes:expr, aarch64_holes: $aarch64_holes:expr
        $(, composition: $composition:ident)?
    } ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum RustLeafRecipe { $( $variant ),+ }

        impl RustLeafRecipe {
            pub(crate) const fn expression(self) -> &'static str {
                match self { $( Self::$variant => $body ),+ }
            }

            pub(crate) const fn parameters(self) -> &'static str {
                match self { $( Self::$variant => $params ),+ }
            }

            pub(crate) const fn result(self) -> &'static str {
                match self { $( Self::$variant => $result ),+ }
            }

            pub(crate) const fn composition(self) -> RecipeComposition {
                match self {
                    $( Self::$variant => recipe_composition!($($composition)?) ),+
                }
            }

        }

        const RUST_LEAF_RECIPES: &[RustLeafRecipe] = &[$(RustLeafRecipe::$variant),+];
        const RUST_LEAF_DECLARATIONS: &[RegionDeclaration] = &[$(
            RegionDeclaration {
                name: $name,
                operations: &[$($op),+],
                abi: DeclAbi::$abi,
                x86_bytes: $x86,
                aarch64_bytes: $aarch64,
                portable_bytes: &[0xC3],
                holes: $holes,
                aarch64_holes: $aarch64_holes,
                entry: 0,
                external_entries: &[0],
            }
        ),+];
    };
}

macro_rules! rust_assembly_catalog {
    ($( $variant:ident {
        name: $name:literal, abi: $abi:ident, ops: [$($op:literal),+],
        x86: $x86:expr, aarch64: $aarch64:expr,
        x86_holes: $x86_holes:expr, aarch64_holes: $aarch64_holes:expr
        $(, bindings: $bindings:expr)?
        $(, outputs: $outputs:expr)?
        $(, continuation: { head: $head:literal, tail: $tail:literal, target: $target:literal })?
        $(, composition: $composition:ident)?
    } ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum RustAssemblyRecipe { $( $variant ),+ }

        impl RustAssemblyRecipe {
            pub(crate) const fn name(self) -> &'static str {
                match self { $( Self::$variant => $name ),+ }
            }

            pub(crate) const fn composition(self) -> RecipeComposition {
                match self {
                    $( Self::$variant => recipe_composition!($($composition)?) ),+
                }
            }

            pub(crate) const fn continuation(self) -> Option<AssemblyContinuation> {
                match self {
                    $( Self::$variant => assembly_continuation!($(($head, $tail, $target))?) ),+
                }
            }

            pub(crate) const fn bindings(self) -> &'static [PhysicalBinding] {
                match self {
                    $( Self::$variant => rust_assembly_bindings!($($bindings)?), )+
                }
            }

            pub(crate) const fn outputs(self) -> &'static [PhysicalOutput] {
                match self {
                    $( Self::$variant => rust_assembly_outputs!($($outputs)?), )+
                }
            }

        }

        const RUST_ASSEMBLY_RECIPES: &[RustAssemblyRecipe] =
            &[$(RustAssemblyRecipe::$variant),+];
        const RUST_ASSEMBLY_DECLARATIONS: &[RegionDeclaration] = &[$(
            RegionDeclaration {
                name: $name,
                operations: &[$($op),+],
                abi: DeclAbi::$abi,
                x86_bytes: $x86,
                aarch64_bytes: $aarch64,
                portable_bytes: &[0xC3],
                holes: $x86_holes,
                aarch64_holes: $aarch64_holes,
                entry: 0,
                external_entries: &[0],
            }
        ),+];
    };
}

macro_rules! rust_assembly_bindings {
    () => {
        &[]
    };
    ($bindings:expr) => {
        $bindings
    };
}

macro_rules! rust_assembly_outputs {
    () => {
        &[]
    };
    ($outputs:expr) => {
        $outputs
    };
}
