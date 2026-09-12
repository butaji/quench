// Declare each plan type once; derive ownership, accounting, and diagnostics.
// Admission order and execution remain explicit in the baseline driver.
macro_rules! native_admission_catalog {
    ($( $variant:ident($plan:ty) => $label:literal ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        enum NativeAdmissionKind {
            $( $variant ),+
        }

        #[derive(Clone)]
        enum NativeAdmission {
            $( $variant(Rc<RefCell<$plan>>) ),+
        }

        impl AdmissionEntry for NativeAdmission {
            fn retained_metadata_bytes(&self) -> usize {
                match self {
                    $( Self::$variant(_) =>
                        crate::stencil_admission_budget::shared_value_bytes::<RefCell<$plan>>()
                    ),+
                }
            }

            fn kind(&self) -> u8 {
                match self {
                    $( Self::$variant(_) => NativeAdmissionKind::$variant as u8 ),+
                }
            }
        }

        impl std::fmt::Debug for NativeAdmission {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(match self {
                    $( Self::$variant(_) => $label ),+
                })
            }
        }
    };
}

native_admission_catalog! {
    Binary(NativeBinaryPlan) => "binary",
    BinarySeries(crate::stencil_region_builder::NativeBinarySeriesPlan) => "binary_series",
    ConstantBinarySeries(crate::stencil_region_builder::NativeConstantBinarySeriesPlan) => "constant_binary_series",
    LoadConst(NativeLoadConstPlan) => "load_const",
    Truthiness(NativeTruthinessPlan) => "truthiness",
    WordBranch(crate::stencil_word_composition::NativeWordBranchPlan) => "word_branch",
    ConstantBranch(crate::stencil_word_composition::NativeWordConstantBranchPlan) => "constant_branch",
    Nullish(NativeNullishPlan) => "nullish",
    Unary(NativeUnaryPlan) => "unary",
    AddChain(NativeAddChainPlan) => "add_chain",
    LocalBinary(crate::stencil_fusion::NativeLocalBinaryPlan) => "local_binary",
    LocalPredicate(crate::stencil_fusion::NativeLocalPredicatePlan) => "local_predicate",
    LocalProperty(crate::stencil_fusion::NativeLocalPropertyPlan) => "local_property",
    NumericDag(crate::stencil_numeric_dag::NativeNumericDagPlan) => "numeric_dag",
    NumberClassify(crate::stencil_number_classify::NativeNumberClassifyPlan) => "number_classify",
    NullishTruthy(crate::stencil_nullish_truthy::NativeNullishTruthyPlan) => "nullish_truthy",
    MissingProperty(crate::stencil_missing_property::NativeMissingPropertyPlan) => "missing_property",
    DenseFill(crate::stencil_dense_array_fill::NativeDenseFillPlan) => "dense_fill",
    IntegerLoop(crate::stencil_numeric_integer_loop::NativeIntegerLoopPlan) => "integer_loop",
    FloatingLoop(crate::stencil_numeric_floating_loop::NativeFloatingLoopPlan) => "floating_loop",
    BitwiseLoop(crate::stencil_numeric_bitwise_loop::NativeBitwiseLoopPlan) => "bitwise_loop",
    IndependentLoop(crate::stencil_numeric_independent_loop::NativeIndependentLoopPlan) => "independent_loop",
    MixedLoop(crate::stencil_numeric_mixed_loop::NativeMixedLoopPlan) => "mixed_loop",
    DenseUpdate(crate::stencil_dense_array_update::NativeDenseUpdatePlan) => "dense_update",
    DenseCopy(crate::stencil_dense_array_copy::NativeDenseCopyPlan) => "dense_copy",
    Reduction(crate::stencil_ordered_reduction::NativeReductionPlan) => "reduction",
    I32Pattern(crate::stencil_i32_pattern::NativeI32PatternPlan) => "i32_pattern",
    LocalAffineSum(crate::stencil_local_affine_sum::NativeLocalAffineSumPlan) => "local_affine_sum",
    LocalRecursiveSum(crate::stencil_local_recursive_sum::NativeLocalRecursiveSumPlan) => "local_recursive_sum",
    CallReturn(crate::stencil_call_return::NativeCallReturnPlan) => "call_return",
    ForwardCall(crate::stencil_forward_call::NativeForwardCallPlan) => "forward_call",
    ForwardPair(crate::stencil_forward_call::NativeForwardPairPlan) => "forward_pair",
    FreshObjectCall(crate::stencil_fresh_object_call::NativeFreshObjectCallPlan) => "fresh_object_call",
    MethodCall(crate::stencil_method_call::NativeMethodCallPlan) => "method_call",
    PropertyPair(crate::stencil_property_pair::NativePropertyPairPlan) => "property_pair",
    PropertyReturnCall(crate::stencil_property_return_call::NativePropertyReturnCallPlan) => "property_return_call",
    PropertyStoreCall(crate::stencil_property_store_call::NativePropertyStoreCallPlan) => "property_store_call",
    PrototypeCall(crate::stencil_prototype_call::NativePrototypeCallPlan) => "prototype_call",
    StringConcat(crate::stencil_string_concat::StringConcatPlan) => "string_concat",
    StringBuiltin(crate::stencil_string_builtin::StringBuiltinPlan) => "string_builtin",
    PropertyNumeric(crate::stencil_property_numeric::PropertyNumericPlan) => "property_numeric",
    Move(NativeMovePlan) => "move",
    LoadLocal(NativeMovePlan) => "load_local",
    StoreLocal(NativeMovePlan) => "store_local",
    StoreProperty(NativePropertyPlan) => "store_property",
    Property(NativePropertyPlan) => "property",
    Dispatch(NativeDispatchPlan) => "dispatch",
    Region(NativeRegionPlan) => "region",
}
