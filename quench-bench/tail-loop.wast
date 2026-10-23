(module
  (func (export "run") (param $n i32) (result i32) (local $i i32)
    local.get $n
    local.set $i
    block
      loop
        local.get $i
        i32.eqz
        br_if 1
        local.get $i
        i32.const 1
        i32.sub
        local.set $i
        br 0
      end
    end
    i32.const 42))
(assert_return (invoke "run" (i32.const 1000000)) (i32.const 42))
