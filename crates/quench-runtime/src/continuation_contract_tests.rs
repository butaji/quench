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
