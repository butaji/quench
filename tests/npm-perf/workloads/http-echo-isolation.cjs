const http = require("node:http");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const requestBody = JSON.stringify({ label: "catalog-item", count: 3, enabled: true });
const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
const server = http.createServer((request, response) => {
  const id = request.url.slice("/records/".length);
  let body = "";
  request.setEncoding("utf8");
  request.on("data", (chunk) => { body += chunk; });
  request.on("end", () => {
    const parsed = JSON.parse(body);
    const payload = JSON.stringify({
      id,
      label: parsed.label,
      count: parsed.count,
      enabled: parsed.enabled,
    });
    response.writeHead(200, {
      "content-type": "application/json",
      "content-length": Buffer.byteLength(payload),
    });
    response.end(payload);
  });
});

function send(port, index) {
  const id = String(index & 63);
  return new Promise((resolve, reject) => {
    const request = http.request({
      host: "127.0.0.1",
      port,
      method: "POST",
      path: `/records/${id}`,
      agent,
      headers: {
        "content-type": "application/json",
        "content-length": Buffer.byteLength(requestBody),
      },
    }, (response) => {
      let payload = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => { payload += chunk; });
      response.on("end", () => resolve({ statusCode: response.statusCode, payload }));
    });
    request.on("error", reject);
    request.end(requestBody);
  });
}

async function main() {
  try {
    await new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    const port = server.address().port;
    for (let index = 0; index < warmups; index++) await send(port, index);

    let bytes = 0;
    let checksum = 0;
    const start = Date.now();
    for (let index = 0; index < rounds; index++) {
      const response = await send(port, index);
      const id = String(index & 63);
      const expected = JSON.stringify({
        id,
        label: "catalog-item",
        count: 3,
        enabled: true,
      });
      if (response.statusCode !== 200 || response.payload !== expected) {
        throw new Error(`HTTP echo mismatch for id ${id}: ${response.statusCode} ${response.payload}`);
      }
      bytes += response.payload.length;
      checksum = (checksum + response.payload.charCodeAt(index % response.payload.length)) >>> 0;
    }
    const elapsedMs = Math.max(1, Date.now() - start);
    console.log(JSON.stringify({
      workload: "http-echo-isolation",
      rounds,
      warmups,
      bytes,
      checksum,
      elapsedMs,
      operationsPerSecond: rounds * 1000 / elapsedMs,
    }));
  } finally {
    agent.destroy();
    await new Promise((resolve) => server.close(resolve));
  }
}

main().catch((error) => {
  console.error(error == null ? "HTTP echo failed with no error value" : error.stack || error);
  process.exitCode = 1;
});
