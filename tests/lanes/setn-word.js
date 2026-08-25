function setX(object, value) {
  object.x = value;
}

const object = { x: 0 };
for (let i = 0; i < 100000; i++) setX(object, i);
if (object.x !== 99999) throw new Error("named word store lost update");
