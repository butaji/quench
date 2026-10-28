const closures = [];
for (let i = 0; i < 3000; i++) {
  const scope = { value: i + 1 };
  with (scope) {
    closures.push(() => value);
  }
}
let checksum = 0;
for (let i = 0; i < closures.length; i++) checksum += closures[i]();
console.log(checksum);
