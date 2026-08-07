{
  if (globalThis.process && globalThis.process.hrtime) {
    globalThis.process.hrtime.bigint ||= () => BigInt(Date.now()) * 1000000n;
  }
}
