function Capture(){return require('util').getCallSites(2).map(site=>[site.functionName,site.scriptName.endsWith('util-callsites-probe.cjs'),site.lineNumber>0,site.columnNumber>0])}
console.log(JSON.stringify(Capture()));
