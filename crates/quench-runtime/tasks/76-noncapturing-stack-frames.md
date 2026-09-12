# 76 — Non-capturing stack-frame representation

Status: planned

Eliminate `Rc<RefCell<Environment>>` only when lowering proves that no nested closure can retain the function frame. Locals remain fixed slots and non-local name lookup begins at the captured outer environment. The final representation must improve the full-suite gate without introducing a per-suite regression.

Two prototypes were measured and rejected:

- A single combined register/local `Vec<Value>` improved the eight-suite aggregate by 2.77% and Earley-Boyer by 5.41%, but regressed Crypto by 11.81% (`reports/stack-frame-clean-ab/comparison.txt`; confirmed at −12.94% in `reports/stack-frame-crypto-ab/comparison.txt`).
- A fixed 16-slot Rust-stack local array separated from the register vector removed the Crypto regression, but changed aggregate by −0.93% and regressed DeltaBlue by 6.59% (`reports/inline-local-frame-ab/comparison.txt`).

Both code paths were removed. Next options are stored per-function slot descriptors without hash lookup, a size-classed frame arena, or true escape-analysis/scalar replacement under [[24-escape-analysis-scalar-replacement]].

Acceptance: five-repetition full-suite A/B with aggregate improvement and no suite below the standing regression floor, plus closure-lifetime and recursive-call correctness tests.
