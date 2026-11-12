//! A construct plan's one dependency: it holds while the objects its proof read keep their
//! values and layouts (`PrototypeValueWatch`).

use super::*;

/// Records `resolution`, where `callee`'s forwarding read `function`, in plan `index`, which
/// then also holds only while `function` and the holder of its `apply` keep their layouts.
pub(super) fn record_forwarded_resolution<H: Host>(vm: &mut Vm<H>, index: usize, function: Value, resolution: ForwardedResolution) {
    vm.lane_stack.construct_plans[index].forwarded = Some(resolution);
    watch_construct_plan(vm, index, false, &[function, resolution.apply_holder]);
}

/// Makes plan `index` hold only while `objects`, which its proof read besides the callee (the
/// callee's own layout, so its `prototype` and [[Prototype]] among them), keep their values and
/// layouts: `renew` starts the plan's dependency, else it adds to it. A write to a slot of any
/// of them, or a layout change anywhere, retires it (`subscribe_prototype_values`), so a use
/// compares no epoch, layout or shape. The plan never holds where the reader table is full.
pub(super) fn watch_construct_plan<H: Host>(vm: &mut Vm<H>, index: usize, renew: bool, objects: &[Value]) {
    use crate::code_dependencies::DependentCode;
    use std::rc::Rc;
    if renew {
        vm.lane_stack.construct_watches[index] = Some(crate::code_dependencies::new_dependency());
    }
    let Some(watch) = vm.lane_stack.construct_watches[index].clone() else { return };
    let code: Rc<dyn DependentCode> = watch.clone();
    for &object in objects {
        vm.heap.mark_prototype(object);
        let shape = vm.heap.function(object).map(|function| function.object.exact_shape())
            .or_else(|| vm.heap.object(object).map(|data| data.exact_shape()));
        if !shape.is_some_and(|shape| crate::heap::subscribe_prototype_values(shape, &code)) {
            watch.retire();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_host::TestHost;
    use crate::Engine;
    use crate::vm::Vm;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn forwarded_construct_plans_retire_when_their_prototype_or_method_changes() {
        // A plan made while hot holds under one dependency: a new method value, a new
        // prototype object and a replaced method each take the next construction off it.
        let source = r#"
          var Class = { create: function () { return function () { this.initialize.apply(this, arguments); }; } };
          var V = Class.create();
          V.prototype = { initialize: function (x, y) { this.x = x; this.y = y; } };
          function run(n) { var s = 0; for (var i = 0; i < n; i++) { var v = new V(i, 2); s += v.x + v.y; } return s; }
          function warm() { for (var k = 0; k < 40; k++) { run(300); } }
          var out = [];
          warm(); out.push(run(10));
          V.prototype.initialize = function (x, y) { this.x = x * 10; this.y = y; };
          out.push(run(10));
          warm();
          V.prototype = { initialize: function (x, y) { this.x = -x; this.y = y; } };
          out.push(run(10));
          warm();
          V.prototype.initialize = function (x, y) { this.x = x; this.y = 100; };
          out.push(run(10));
          print(out.join());
        "#;
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = Engine::specialize(source, "retiring-construct-plans.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["65,470,-25,1045"]);
    }
}
