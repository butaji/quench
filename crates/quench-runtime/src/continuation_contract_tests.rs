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
