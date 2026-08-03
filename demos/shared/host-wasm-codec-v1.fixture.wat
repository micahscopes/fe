(module
  (import "fe:fixture" "send"
    (func $send (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (global $cursor (mut i32) (i32.const 512))

  (func (export "cabi_alloc") (param $size i32) (result i32)
    (local $ptr i32)
    global.get $cursor
    local.tee $ptr
    local.get $size
    i32.add
    global.set $cursor
    local.get $ptr)

  (func (export "cabi_realloc")
    (param $old_ptr i32) (param $old_size i32) (param $align i32) (param $new_size i32)
    (result i32)
    (local $ptr i32)
    global.get $cursor
    local.get $align
    i32.const 1
    i32.sub
    i32.add
    local.get $align
    i32.const 1
    i32.sub
    i32.const -1
    i32.xor
    i32.and
    local.tee $ptr
    local.get $new_size
    i32.add
    global.set $cursor
    local.get $ptr)

  (func (export "cabi_post_fixture_send") (param i32) (param i32) (param i32))

  (func (export "run") (param i32 i32 i32 i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    local.get 4
    call $send)
)
