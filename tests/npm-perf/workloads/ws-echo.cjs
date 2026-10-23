const { WebSocket, WebSocketServer } = require("ws");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 1000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 50);
const payload = Buffer.from("quench-websocket-echo/0123456789abcdef");

function exchange(socket, index) {
  return new Promise((resolve, reject) => {
    const expected = `${index}:`;
    const message = Buffer.concat([Buffer.from(expected), payload]);
    const onError = (error) => {
      socket.off("message", onMessage);
      reject(error);
    };
    const onMessage = (value) => {
      socket.off("error", onError);
      socket.off("message", onMessage);
      const received = Buffer.isBuffer(value) ? value : Buffer.from(value);
      if (!received.equals(message)) {
        reject(new Error(`WebSocket echo mismatch at ${index}`));
      } else {
        resolve(received);
      }
    };
    socket.once("error", onError);
    socket.once("message", onMessage);
    socket.send(message, (error) => {
      if (error) reject(error);
    });
  });
}

async function main() {
  const server = new WebSocketServer({ port: 0, host: "127.0.0.1" });
  server.on("connection", (socket) => {
    socket.on("message", (message) => socket.send(message));
  });
  await new Promise((resolve, reject) => {
    server.once("listening", resolve);
    server.once("error", reject);
  });

  const address = server.address();
  const client = new WebSocket(`ws://127.0.0.1:${address.port}`);
  try {
    await new Promise((resolve, reject) => {
      client.once("open", resolve);
      client.once("error", reject);
    });

    for (let index = 0; index < warmups; index += 1) await exchange(client, index);
    let bytes = 0;
    let checksum = 0;
    const start = Date.now();
    for (let index = 0; index < rounds; index += 1) {
      const echoed = await exchange(client, index);
      bytes += echoed.length;
      checksum = (checksum + echoed[index % echoed.length]) >>> 0;
    }
    const elapsedMs = Math.max(1, Date.now() - start);
    const clientClosed = new Promise((resolve) => client.once("close", resolve));
    client.close();
    await clientClosed;
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));

  console.log(JSON.stringify({
      workload: "ws-echo",
      package: "ws@8.18.3",
      rounds,
      warmups,
      cases: ["client-server-upgrade", "sequential-echo", "binary-payload"],
      bytes,
      checksum,
      elapsedMs,
      operationsPerSecond: rounds * 1000 / elapsedMs,
    }));
  } catch (error) {
    client.terminate();
    await new Promise((resolve) => server.close(resolve));
    throw error;
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
