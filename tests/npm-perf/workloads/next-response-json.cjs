const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const payload = {
  route: 2,
  params: { id: "8421" },
  result: { status: "ready", records: 12 },
};
const encoded = JSON.stringify(payload);
const bytesPerResponse = Buffer.byteLength(encoded);
const checksumPerResponse = [...encoded].reduce(
  (sum, character) => (sum + character.charCodeAt(0)) >>> 0,
  0,
);

function createResponse() {
  const response = Response.json(payload, { status: 200 });
  if (response.status !== 200 || !response.body ||
    !response.headers.get("content-type")?.startsWith("application/json")) {
    throw new Error("Response.json output metadata mismatch");
  }
}

function main() {
  for (let index = 0; index < warmups; index += 1) createResponse();
  let bytes = 0;
  let checksum = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    createResponse();
    bytes += bytesPerResponse;
    checksum = (checksum + checksumPerResponse) >>> 0;
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "next-response-json",
    package: "next@16.4.3",
    rounds,
    warmups,
    cases: ["Response.json", "JSON body serialization", "body-stream construction"],
    bytes,
    checksum,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

try {
  main();
} catch (error) {
  console.error(error);
  process.exitCode = 1;
}
