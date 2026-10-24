function readValue(object) {
  return object.value;
}

const sameShapeA = { value: 2 };
const sameShapeB = { value: 9 };
const alternateShapeA = { extra: true, value: 13 };
const alternateShapeB = { value: 21, extra: true };
const receivers = [sameShapeA, sameShapeB, alternateShapeA, alternateShapeB];

for (let index = 0; index < 32768; index++) {
  readValue(receivers[index & 3]);
}

sameShapeA.value = 101;
alternateShapeB.value = 34;
const values = receivers.map(readValue);
console.log(JSON.stringify(values));
