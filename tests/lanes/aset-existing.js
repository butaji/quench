const values = [null, null, null, null];
for (let i = 0; i < 250000; i++) values[i & 3] = i;
if (values[0] !== 249996 || values[3] !== 249999) {
  throw new Error("existing array store result");
}
