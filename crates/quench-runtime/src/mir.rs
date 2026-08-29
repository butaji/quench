//! MIR is specialised HIR. The result is still HIR (NIR, FIR, or DIR).

use crate::hir::HirFunc;

/// Specialised function the interpreter runs. Same representation as HIR.
pub type MirFunc = HirFunc;

/// Derive MIR from HIR. Native-proven (NIR) code does not change shape.
pub fn specialise(func: HirFunc) -> MirFunc {
    func
}

#[cfg(test)]
mod tests {
    use super::specialise;
    use crate::hir::{HirFunc, Inst, Ty};

    #[test]
    fn native_ir_is_hir_subset() {
        use crate::hir::Inst;
        use crate::layer::Layer;
        assert_eq!(Inst::ConstI32 { dst: 0, val: 1 }.ir(), Layer::Native);
        assert_eq!(
            Inst::Guard {
                dst: 0,
                src: 1,
                kind: crate::layer::GuardKind::I32,
            }
            .ir(),
            Layer::Fast
        );
        assert_eq!(Inst::BoxToDynamic { dst: 0, src: 1 }.ir(), Layer::Dynamic);
        assert_eq!(Layer::Native.join(Layer::Fast), Layer::Fast);
        assert_eq!(Layer::Fast.join(Layer::Dynamic), Layer::Dynamic);
    }

    #[test]
    fn native_specialise_is_identity() {
        let func = HirFunc {
            params: Box::new([]),
            results: Box::new([Ty::NATIVE_I32]),
            locals: Box::new([]),
            nregs: 1,
            code: Box::new([
                Inst::ConstI32 { dst: 0, val: 1 },
                Inst::Return {
                    srcs: Box::new([0]),
                },
            ]),
        };
        let mir = specialise(func.clone());
        assert_eq!(mir, func);
    }
}
