use crate::execute::VmError;
use crate::value::Value;

fn execute(source: &str) -> Result<Value, VmError> {
    let program = crate::reduce::reduce_source(source).expect("continuation source reduces");
    crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
}

fn run_sync(source: &str) {
    let _scope = crate::with_scope::FunctionGuard::isolate();
    assert_eq!(
        execute(source).expect("continuation source runs"),
        Value::Undefined
    );
}

fn run_async(source: &str) {
    let _scope = crate::with_scope::FunctionGuard::isolate();
    crate::module_bindings::reset_module_jobs();
    crate::take_unhandled_rejections();
    assert_eq!(
        execute(source).expect("async source starts"),
        Value::Undefined
    );
    crate::drain_promise_jobs();
    let errors = crate::take_unhandled_rejections();
    assert!(
        !crate::has_pending_promise_jobs(),
        "async jobs did not drain"
    );
    assert_eq!(errors.len(), 1, "async continuation outcome: {errors:?}");
    assert_eq!(
        errors[0].1,
        Value::String("__continuation_contract_done__".into())
    );
    crate::module_bindings::reset_module_jobs();
}

#[test]
fn regression_bound_has_instance_exhaustion_is_catchable_and_recovers() {
    std::thread::Builder::new()
        .stack_size(crate::WORKER_STACK_SIZE)
        .spawn(|| {
            let mut reservations = Vec::new();
            while let Ok(guard) = quench_stack::StackGuard::enter() {
                reservations.push(guard);
            }
            let stress_depth = reservations.len() + 1;
            drop(reservations);
            let source = r#"
                var intrinsicRangeError = RangeError;
                RangeError = function () { throw "replaced"; };
                var bound = function Leaf() {}, owners = [], boundStressDepth = BOUND_STRESS_DEPTH;
                for (var i = 0; i < boundStressDepth; i++) {
                    owners.push(bound);
                    bound = bound.bind(null);
                    Object.defineProperty(bound, "name", { value: "" });
                }
                var caught = false;
                try { Function.prototype[Symbol.hasInstance].call(bound, {}); }
                catch (error) {
                    if (!(error instanceof intrinsicRangeError) ||
                        error.message !== "Maximum call stack size exceeded") throw "bad stack error";
                    caught = true;
                }
                if (!caught) throw "bound chain did not exhaust";
                bound = null;
                while (owners.length) owners.pop();
                function Ordinary() {}
                var instance = new Ordinary();
                if (!Function.prototype[Symbol.hasInstance].call(Ordinary.bind(null), instance)) {
                    throw "ordinary bound instance or budget recovery failed";
                }
            "#.replace("BOUND_STRESS_DEPTH", &stress_depth.to_string());
            run_sync(&source);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn async_loops_resume_every_nested_and_control_phase() {
    run_async(
        r#"
        async function verify() {
          var sum=0;
          for(var i=0;i<2;i++) for(var j=0;j<2;j++) sum+=await 1;
          if(sum!==4) throw "nested await skipped a continuation";
          var test=[],k=0; while(await(k<3)){test.push(k++);}
          var update=[]; for(var u=0;u<3;u=await(u+1)) update.push(u);
          var init=[]; for(var n=await 0;n<3;n++) init.push(n);
          var post=[],p=0; do{post.push(p++);}while(await(p<3));
          if(test.join()!=="0,1,2" || update.join()!=="0,1,2" ||
             init.join()!=="0,1,2" || post.join()!=="0,1,2") {
            throw "await loop phase resumed at the wrong point";
          }
        }
        verify().then(function(){throw "__continuation_contract_done__";});
        "#,
    );
}

#[test]
fn generators_preserve_nested_progress_finally_and_return() {
    run_sync(
        r#"
        function step(result,value,done){
          if(result.value!==value || result.done!==done) throw "bad generator step";
        }
        function* nested(){for(var i=0;i<2;i++)for(var j=0;j<2;j++)yield i*10+j;}
        var g=nested(); step(g.next(),0,false); step(g.next(),1,false);
        step(g.next(),10,false); step(g.next(),11,false); step(g.next(),undefined,true);
        var log=[]; function* guarded(){try{for(var k=0;k<3;k++)yield k;}finally{log.push(1);}}
        var h=guarded(); step(h.next(),0,false); step(h.next(),1,false);
        step(h.next(),2,false); step(h.next(),undefined,true);
        if(log.length!==1) throw "generator finally did not run exactly once";
        function* returning(){for(var q=0;q<3;q++){yield q;if(q===1)return 99;}}
        var r=returning(); step(r.next(),0,false); step(r.next(),1,false);
        step(r.next(),99,true); step(r.next(),undefined,true);
        "#,
    );
}

#[test]
fn throwing_for_of_closes_generator_before_outer_catch() {
    run_sync(
        r#"
        var closed=0,caught=-1;
        function* values(){try{yield 64;}finally{closed++;}}
        try{for(var value of values())throw value;}catch(error){caught=error;}
        if(caught!==64 || closed!==1) throw "iterator throw/close order";
        "#,
    );
}

#[test]
fn injected_generator_return_runs_finally_and_preserves_value() {
    run_sync(
        r#"
        var closed=0;
        function* values(){try{yield 1;}finally{closed++;}}
        var iterator=values(),first=iterator.next(),last=iterator.return(42);
        if(first.value!==1 || first.done || last.value!==42 || !last.done || closed!==1) {
          throw "generator return/finally order";
        }
        "#,
    );
}

#[test]
fn generator_suspends_inside_try_before_close() {
    run_sync(
        r#"
        function* values(){try{yield 1;}finally{}}
        var first=values().next();
        if(first.value!==1 || first.done) throw "generator did not suspend in try";
        "#,
    );
}

#[test]
fn generator_try_catch_loop_resumes_each_yield() {
    run_sync(
        r#"
        function* values(){for(var i=0;i<8;i++){try{if(i===3)throw i;yield i;}catch(e){yield e+10;}}}
        var result=[],iterator=values(),step;
        while(!(step=iterator.next()).done) result.push(step.value);
        if(result.join(",")!=="0,1,2,13,4,5,6,7") throw "try/catch loop resume";
        "#,
    );
}

#[test]
fn nested_generator_for_of_resumes_source_and_consumer() {
    run_sync(
        r#"
        function* source(){for(var i=0;i<4;i++)yield i+7;}
        function* mapped(input){for(var value of input)yield value*3;}
        var total=0;for(var value of mapped(source()))total+=value;
        if(total!==102) throw "nested generator composition";
        "#,
    );
}

#[test]
fn promoted_loop_body_retains_executed_suspension_pc() {
    run_sync(
        r#"
        function* values(){for(var i=0;i<64;i++)yield i;}
        for(var round=0;round<40;round++) {
          var iterator=values(),step,total=0,count=0;
          while(!(step=iterator.next()).done){total+=step.value;count++;}
          if(total!==2016 || count!==64) throw "promoted suspension lost its pc";
        }
        "#,
    );
}

#[test]
fn promoted_await_body_retains_executed_suspension_pc() {
    run_async(
        r#"
        async function verify(){
          for(var round=0;round<40;round++){
            var total=0;
            for(var i=0;i<64;i++) total+=await 1;
            if(total!==64) throw "promoted await lost its pc";
          }
        }
        verify().then(function(){throw "__continuation_contract_done__";});
        "#,
    );
}

#[test]
fn async_branch_resumes_suffix_after_nested_loops() {
    run_async(
        r#"
        async function invoke(){return await 1;}
        async function window(count){
          var calls=0,value;
          do{value=await invoke();calls++;}while(calls<count);
          return value;
        }
        async function verify(){
          var log=[],mode="throughput";
          if(mode==="throughput"){
            for(var i=0;i<4;i++) log.push(await window(64));
            for(var j=0;j<3;j++) log.push(await window(64));
          }
          log.push("after");
          if(log.join()!=="1,1,1,1,1,1,1,after") {
            throw "branch suffix was skipped after nested loops";
          }
          return log.join();
        }
        verify().then(function(value){
          if(value!=="1,1,1,1,1,1,1,after") throw "async return was lost";
          throw "__continuation_contract_done__";
        });
        "#,
    );
}

#[test]
fn suspended_generator_keeps_captured_binding_during_collection() {
    run_sync(
        r#"
        function make(){
          var f=function(){return 42;};
          var dead1=function(){return f();},dead2=function(){return dead1();};
          return (function*(){yield 1;return f();})();
        }
        var g=make(),first=g.next();
        if(first.value!==1 || first.done) throw "generator did not suspend";
        for(var i=0;i<4096;i++){var a={},b={};a.peer=b;b.peer=a;}
        var last=g.next();
        if(last.value!==42 || !last.done) throw "suspended capture was reclaimed";
        "#,
    );
}

#[test]
fn rejected_await_preserves_bindings_and_runs_catch_finally_once() {
    run_async(
        r#"
        async function verify() {
          var log=[],captured=function(){return 42;};
          try {
            log.push("before");
            await Promise.reject("boom");
            log.push("unreachable");
          } catch(error) {
            log.push("catch:"+error+":"+captured());
          } finally {
            log.push("finally:"+captured());
          }
          if(log.join("|")!=="before|catch:boom:42|finally:42") {
            throw "rejected await replayed or lost its continuation";
          }
        }
        verify().then(function(){throw "__continuation_contract_done__";});
        "#,
    );
}

#[test]
fn async_frames_compose_across_depth_sequence_and_labels() {
    run_async(
        r#"
        async function verify() {
          var sum=0,log=[];
          for(var i=0;i<2;i++) for(var j=0;j<2;j++)
            for(var k=0;k<2;k++) sum+=await 1;
          for(var a=0;a<2;a++) log.push("a"+await a);
          for(var b=0;b<2;b++) log.push("b"+await b);
          outer: for(var x=0;x<3;x++) for(var y=0;y<2;y++) {
            if(y===0) continue;
            log.push("x"+await x);
            if(x===1) continue outer;
          }
          if(sum!==8 || log.join("|")!=="a0|a1|b0|b1|x0|x1|x2") {
            throw "structured async frames lost order or an outer suffix";
          }
        }
        verify().then(function(){throw "__continuation_contract_done__";});
        "#,
    );
}

#[test]
fn generator_resumes_init_test_update_and_finally_phases() {
    run_sync(
        r#"
        function* phases(log) {
          try {
            for(var i=yield "init"; yield i<2; i=yield i+1) log.push(i);
            return log.join(",");
          } finally { log.push("finally"); }
        }
        var log=[],g=phases(log),r;
        r=g.next(); if(r.value!=="init"||r.done) throw "init";
        r=g.next(0); if(r.value!==true||r.done) throw "test0";
        r=g.next(true); if(r.value!==1||r.done) throw "update0";
        r=g.next(1); if(r.value!==true||r.done) throw "test1";
        r=g.next(true); if(r.value!==2||r.done) throw "update1";
        r=g.next(2); if(r.value!==false||r.done) throw "test2";
        r=g.next(false); if(r.value!=="0,1"||!r.done) throw "return";
        if(log.join(",")!=="0,1,finally") throw "finally";
        "#,
    );
}

#[test]
fn regression_recursive_transitions_throw_and_recover() {
    std::thread::Builder::new()
        .name("legacy-recursive-transitions".into())
        .stack_size(crate::WORKER_STACK_SIZE)
        .spawn(|| {
            run_sync(r#"
                var checks = 0;
                var intrinsicRangeError = RangeError;
                RangeError = function () { throw "guest RangeError constructor called"; };
                function check(operation) {
                    checks++;
                    var caught = false;
                    try { operation(); }
                    catch (error) {
                        if (!(error instanceof intrinsicRangeError) ||
                            error.message !== "Maximum call stack size exceeded") throw "bad stack error " + checks;
                        caught = true;
                    }
                    if (!caught) throw "recursion returned";
                    if ((function () { return 42; })() !== 42) throw "budget did not recover";
                }
                check(function () { function f() { return 1 + f(); } f(); });
                check(function () { var object = { get value() { return this.value; } }; object.value; });
                check(function () { var object = { set value(value) { this.value = value; } }; object.value = 1; });
                check(function () { function C() { new C(); } new C(); });
                check(function () {
                    var proxy = new Proxy({}, { get: function (target, key, receiver) { return receiver[key]; } });
                    proxy.value;
                });
                check(function () { var object = { toString: function () { return String(this); } }; String(object); });
                check(function () { JSON.parse({ toString: function () { return JSON.parse(this); } }); });
                var result = 0;
                with ({}) {
                    function tail(n) { "use strict"; if (n) return tail(n - 1); return 42; }
                    result = tail(10000);
                }
                if (result !== 42) throw "tail replacement consumed the stack budget";
            "#);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn regression_legacy_dynamic_compiler_exhaustion_is_catchable_and_recovers() {
    std::thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(|| {
        run_sync(r#"
            var intrinsicRangeError = RangeError;
            RangeError = function () { throw "guest RangeError constructor called"; };
            var compilerStressDepth = 20000;
            var sources = [
                '('.repeat(compilerStressDepth) + '1' + ')'.repeat(compilerStressDepth),
                Array(compilerStressDepth).fill('1').join('+')
            ];
            for (var index = 0; index < sources.length; index++) {
                var source = sources[index];
                for (var mode = 0; mode < 3; mode++) {
                    var caught = false;
                    try {
                        if (mode === 0) eval(source);
                        else if (mode === 1) (0, eval)(source);
                        else Function('return ' + source);
                    } catch (error) {
                        if (!(error instanceof intrinsicRangeError) ||
                            error.message !== 'Maximum call stack size exceeded') throw 'wrong compiler error';
                        caught = true;
                    }
                    if (!caught) throw 'missing compiler exhaustion';
                    if (eval('1 + 2') !== 3) throw 'compiler budget did not recover';
                }
            }
        "#);
    }).unwrap().join().unwrap();
}
