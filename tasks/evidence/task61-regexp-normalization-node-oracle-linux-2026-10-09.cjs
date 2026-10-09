const cases = [
  { aliases: ["Beria_Erfe", "Berf"], cp: 0x16ea0 },
  { aliases: ["Sidetic", "Sidt"], cp: 0x10940 },
  { aliases: ["Tai_Yo", "Tayo"], cp: 0x1e6c0 },
  { aliases: ["Tolong_Siki", "Tols"], cp: 0x11db0 },
];
const result = [];
for (const item of cases) {
  const character = String.fromCodePoint(item.cp);
  for (const alias of item.aliases) {
    const positive = new RegExp(`\\p{Script=${alias}}`, "u");
    const negative = new RegExp(`\\P{Script=${alias}}`, "u");
    result.push({ alias, positive: positive.test(character), negative: negative.test(character) });
  }
}
console.log(JSON.stringify(result));
