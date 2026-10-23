(module
  (func $run (export "run") (param $n i32) (result i32)
    local.get $n
    i32.eqz
    if (result i32)
      i32.const 42
    else
      local.get $n
      i32.const 1
      i32.sub
      return_call $run
    end))
(assert_return (invoke "run" (i32.const 1000000)) (i32.const 42))
