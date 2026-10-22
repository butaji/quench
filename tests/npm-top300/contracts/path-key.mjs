import assert from 'node:assert/strict';
export function exercise(api) {
  const cases=[[{platform:'linux',env:{Path:'x'}},'PATH'],[{platform:'darwin',env:{}},'PATH'],[{platform:'win32',env:{}},'Path'],[{platform:'win32',env:{PATH:'x'}},'PATH'],[{platform:'win32',env:{Path:'x',PATH:'y'}},'PATH'],[{platform:'win32',env:{PATH:'x',pAtH:'y'}},'pAtH']];
  for(const [options,expected] of cases)assert.equal(api(options),expected);
  return {package:'path-key',assertions:cases.length,checks:{'':cases.length},scope:'Finite platform and environment key casing corpus; arbitrary inputs remain obligations'};
}
