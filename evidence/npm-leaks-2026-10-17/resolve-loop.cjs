const path=require('path'); const root=path.join(process.cwd(),'node_modules');
const r=()=>(process.memoryUsage.rss()/1048576).toFixed(1);
console.log('start', r());
for (const n of [1000, 2000, 4000]) { for (let i=0;i<n;i++) require.resolve('typescript', {paths:[root]}); console.log(n, r()); }
