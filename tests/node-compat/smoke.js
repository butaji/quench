if (global !== globalThis) throw new Error('global alias');
if (Buffer.from('6869', 'hex').toString() !== 'hi') throw new Error('Buffer');
if (process.argv[0] !== 'quench-node') throw new Error('process.argv');
process.nextTick(() => {});
