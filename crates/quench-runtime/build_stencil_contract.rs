#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeclAbi {
    Scalar,
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
    PropertyGuard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeclOp {
    Unknown,
    Add,
    Sub,
    Mul,
    Div,
    AddConst,
    Return,
}

fn parse_decl_op(name: &str) -> DeclOp {
    match name {
        "Add" => DeclOp::Add,
        "Sub" => DeclOp::Sub,
        "Mul" => DeclOp::Mul,
        "Div" => DeclOp::Div,
        "AddConst" => DeclOp::AddConst,
        "Return" => DeclOp::Return,
        _ => DeclOp::Unknown,
    }
}

macro_rules! rust_leaf_catalog {
    ($( $variant:ident { ops: [$($op:ident),+], params: $params:literal, expression: $expression:literal } ),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub(crate) enum RustLeafRecipe { $( $variant ),+ }

        impl RustLeafRecipe {
            pub(crate) const fn expression(self) -> &'static str {
                match self { $( Self::$variant => $expression ),+ }
            }

            pub(crate) const fn parameters(self) -> &'static str {
                match self { $( Self::$variant => $params ),+ }
            }
        }

        fn recipe_for_ops(ops: &[DeclOp]) -> Option<RustLeafRecipe> {
            match ops {
                $( [$(DeclOp::$op),+] => Some(RustLeafRecipe::$variant), )+
                _ => None,
            }
        }
    };
}

rust_leaf_catalog! {
    Add { ops: [Add, Return], params: "a: f64, b: f64", expression: "a + b" },
    Sub { ops: [Sub, Return], params: "a: f64, b: f64", expression: "a - b" },
    Mul { ops: [Mul, Return], params: "a: f64, b: f64", expression: "a * b" },
    Div { ops: [Div, Return], params: "a: f64, b: f64", expression: "a / b" },
    AddConst { ops: [AddConst, Return], params: "a: f64, b: f64", expression: "a + b" },
    AddChain { ops: [Add, Add], params: "a: f64, b: f64, c: f64", expression: "(a + b) + c" },
}

fn typed_decl_ops(operations: &[&str]) -> Option<[DeclOp; 4]> {
    if operations.len() > 4 {
        return None;
    }
    let mut typed = [DeclOp::Unknown; 4];
    for (index, operation) in operations.iter().enumerate() {
        typed[index] = parse_decl_op(operation);
    }
    Some(typed)
}

pub(crate) fn rust_leaf_recipe(declaration: &RegionDeclaration) -> Option<RustLeafRecipe> {
    if declaration.abi != DeclAbi::Scalar {
        return None;
    }
    let typed = typed_decl_ops(declaration.operations)?;
    recipe_for_ops(&typed[..declaration.operations.len()])
}
