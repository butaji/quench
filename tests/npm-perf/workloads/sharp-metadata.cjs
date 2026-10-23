const sharp = require("sharp");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const width = 256;
const height = 256;
const channels = 3;
const raw = Buffer.allocUnsafe(width * height * channels);
for (let index = 0; index < raw.length; index += 1) raw[index] = (index * 29 + 17) & 0xff;

async function main() {
  const fixture = await sharp(raw, { raw: { width, height, channels } }).png().toBuffer();
  async function metadata() {
    const result = await sharp(fixture).metadata();
    if (result.width !== width || result.height !== height || result.format !== "png") {
      throw new Error("sharp metadata output mismatch");
    }
    return result;
  }
  for (let index = 0; index < warmups; index += 1) await metadata();
  let checksum = 0;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const result = await metadata();
    bytes += fixture.length;
    checksum = (checksum + result.width + result.height + result.channels) >>> 0;
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "sharp-metadata",
    package: `sharp@${sharp.versions.sharp}`,
    rounds,
    warmups,
    cases: ["encoded-png-input", "metadata", "dimensions-and-channels"],
    bytes,
    checksum,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
