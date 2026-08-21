(async () => {
const { setTimeout, setImmediate, setInterval } = require('node:timers/promises');

if (await setTimeout(1, 'fulfilled') !== 'fulfilled') throw new Error('timeout did not fulfill');
const controller = new AbortController();
const reason = new Error('fixture reason');
const aborted = setTimeout(50, undefined, { signal: controller.signal });
controller.abort(reason);
try {
  await aborted;
  throw new Error('aborted timeout fulfilled');
} catch (error) {
  if (error.name !== 'AbortError' || error.cause !== reason) throw error;
}
const immediate = await setImmediate('immediate', { ref: false });
if (immediate !== 'immediate') throw new Error('immediate did not fulfill');
const intervalController = new AbortController();
const interval = setInterval(1, 'tick', { signal: intervalController.signal, ref: false });
if ((await interval.next()).value !== 'tick') throw new Error('interval did not yield');
intervalController.abort(reason);
try { await interval.next(); throw new Error('interval did not abort'); } catch (error) {
  if (error.name !== 'AbortError' || error.cause !== reason) throw error;
}
console.log('timers/promises fixture passed');
})().catch((error) => { console.error(error); process.exitCode = 1; });
