const http = require("node:http");
const { Pool } = require("../node_modules/undici");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const concurrency = Number(process.env.QUENCH_BENCH_CONCURRENCY || 4);

async function main() {
  if (!Number.isSafeInteger(concurrency) || concurrency < 1) {
    throw new Error("QUENCH_BENCH_CONCURRENCY must be a positive integer");
  }
  const server = http.createServer((request, response) => {
    const id = request.url.slice("/records/".length);
    const body = `{"id":"${id}","label":"catalog-item","count":3,"enabled":true}`;
    response.writeHead(200, {
      "content-type": "application/json",
      "content-length": String(Buffer.byteLength(body)),
      "cache-control": "no-store",
    });
    response.end(body);
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const { port } = server.address();
  const pool = new Pool(`http://127.0.0.1:${port}`, { connections: concurrency });

  async function request(index) {
    const id = String(index & 255);
    const response = await pool.request({ method: "GET", path: `/records/${id}` });
    const body = await response.body.text();
    const expected = `{"id":"${id}","label":"catalog-item","count":3,"enabled":true}`;
    if (response.statusCode !== 200 || body !== expected) {
      throw new Error(`Undici response mismatch for id ${id}: ${response.statusCode} ${body}`);
    }
    return body;
  }

  async function batch(start, count) {
    const responses = await Promise.all(Array.from({ length: count }, (_, offset) => request(start + offset)));
    let checksum = 0;
    let bytes = 0;
    for (let offset = 0; offset < responses.length; offset += 1) {
      const body = responses[offset];
      checksum = (checksum + body.charCodeAt((start + offset) % body.length)) >>> 0;
      bytes += body.length;
    }
    return { checksum, bytes };
  }

  try {
    for (let index = 0; index < warmups; index += concurrency) {
      await batch(index, Math.min(concurrency, warmups - index));
    }
    let checksum = 0;
    let bytes = 0;
    const start = Date.now();
    for (let index = 0; index < rounds; index += concurrency) {
      const count = Math.min(concurrency, rounds - index);
      const result = await batch(index, count);
      checksum = (checksum + result.checksum) >>> 0;
      bytes += result.bytes;
    }
    const elapsedMs = Math.max(1, Date.now() - start);
    console.log(JSON.stringify({
      workload: "undici-pool",
      package: "undici@7.30.1",
      rounds,
      warmups,
      concurrency,
      bytes,
      checksum,
      elapsedMs,
      operationsPerSecond: rounds * 1000 / elapsedMs,
    }));
  } finally {
    await pool.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

main().catch((error) => {
  console.error(error == null ? "Undici workload failed with no error value" : error.stack || error);
  process.exitCode = 1;
});
