use super::tests::osr_run;

/// A method call in a loop region calls the method its site reads, and a prototype's method replaced after the region
/// was built is the one the next calls run: the site read's value (the callee the direct row checks) follows the
/// prototype's slot, so the replaced method's result is seen from the change on.
#[test]
fn a_replaced_prototype_method_is_called_after_the_region_was_built() {
    let (output, _) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "function Proto() {}
         Proto.prototype.m = function (x) { return x + 1; };
         function run(n) {
           var o = new Proto(), s = 0;
           for (var i = 0; i < n; i++) {
             if (i === 150000) Proto.prototype.m = function (x) { return x + 2; };
             s = (s + o.m(i & 1023)) | 0;
           }
           return s;
         }
         print(run(200000));", "run"));
    let expected: i64 = (0..200_000i64).map(|i| (i & 1023) + if i < 150_000 { 1 } else { 2 }).sum();
    assert_eq!(output, vec![expected.to_string()]);
}
