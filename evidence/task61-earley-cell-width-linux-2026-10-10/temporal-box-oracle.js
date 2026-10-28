const instant = Temporal.Instant.from('1999-12-31T23:59:59.123456789Z');
const zdt = Temporal.ZonedDateTime.from('2024-03-09T12:34:56.123456789-05:00[America/New_York]');
const plusDay = zdt.add({ days: 1, hours: 2, nanoseconds: 321 });
const previousDay = zdt.subtract({ days: 1, minutes: 7 });
const roundHour = zdt.round({ smallestUnit: 'hour', roundingMode: 'halfExpand' });
const dstStart = Temporal.ZonedDateTime.from('2024-03-09T12:00-05:00[America/New_York]').add({ days: 1 });
const values = {
  instant: [instant.epochNanoseconds.toString(), instant.toString(), instant.add({ nanoseconds: 876543211 }).toString()],
  zdt: [zdt.epochNanoseconds.toString(), zdt.toString(), zdt.offset, zdt.hoursInDay, zdt.year, zdt.month, zdt.day, zdt.hour],
  plusDay: [plusDay.epochNanoseconds.toString(), plusDay.toString(), plusDay.until(zdt).toString()],
  previousDay: [previousDay.epochNanoseconds.toString(), previousDay.toString()],
  roundHour: [roundHour.epochNanoseconds.toString(), roundHour.toString()],
  dstStart: [dstStart.epochNanoseconds.toString(), dstStart.toString(), dstStart.hoursInDay],
  conversions: [zdt.toInstant().toString(), zdt.toPlainDate().toString(), zdt.toPlainTime().toString(), zdt.withTimeZone('UTC').toString()],
  comparisons: [zdt.equals(zdt), Temporal.ZonedDateTime.compare(zdt, plusDay), Temporal.Instant.compare(instant, zdt.toInstant())],
};
console.log(JSON.stringify(values));
