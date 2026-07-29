// Exact match of the first part of the real test
function CreateRab(buffer_byte_length, ctor) {
  const rab = CreateResizableArrayBuffer(buffer_byte_length, 2 * buffer_byte_length);
  let ta_write = new ctor(rab);
  for (let i = 0; i < buffer_byte_length / ctor.BYTES_PER_ELEMENT; ++i) {
    ta_write[i] = MayNeedBigInt(ta_write, i % 128);
  }
  return rab;
}

// Just Uint8Array 
var ctor = Uint8Array;
var no_elements = 10;
var offset = 2;
var buffer_byte_length = no_elements * ctor.BYTES_PER_ELEMENT;
var byte_offset = offset * ctor.BYTES_PER_ELEMENT;

// ta1 test
var rab = CreateRab(buffer_byte_length, ctor);
var ta1 = new ctor(rab, 0, 3);
TestIterationAndResize(ta1, [0, 1, 2], rab, 2, buffer_byte_length / 2);
print("ta1 done");

// ta2 test - should throw TypeError
rab = CreateRab(buffer_byte_length, ctor);
var ta2 = new ctor(rab, 0, 3);
assert.throws(TypeError, () => {
    TestIterationAndResize(ta2, null, rab, 2, 1);
});
print("ta2 passed");
