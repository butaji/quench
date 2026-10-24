function readValue(object) {
  return object.value;
}

const sameShapeA = { value: 2 };
const sameShapeB = { value: 9 };
const alternateShapeA = { extra: true, value: 13 };
const alternateShapeB = { value: 21, extra: true };
const receivers = [sameShapeA, sameShapeB, alternateShapeA, alternateShapeB];
const firstPrototype = { value: 55 };
const inherited = Object.create(firstPrototype);

for (let index = 0; index < 32768; index++) {
  readValue(receivers[index & 3]);
  readValue(inherited);
}

sameShapeA.value = 101;
alternateShapeB.value = 34;
Object.setPrototypeOf(inherited, { value: 89 });
const values = receivers.map(readValue);
const inheritedValue = readValue(inherited);
console.log(JSON.stringify({ values, inheritedValue }));
