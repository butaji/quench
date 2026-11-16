function mm(A,B,C,n){for(let i=0;i<n;i++)for(let j=0;j<n;j++){let s=0;for(let k=0;k<n;k++)s+=A[i*n+k]*B[k*n+j];C[i*n+j]=s;}}
const n=128,A=new Float64Array(n*n),B=new Float64Array(n*n),C=new Float64Array(n*n);
for(let i=0;i<n*n;i++){A[i]=(i%13)*0.5;B[i]=(i%7)-3;}
for(let r=0;r<10;r++)mm(A,B,C,n);
let t=0;for(let i=0;i<n*n;i++)t+=C[i];console.log(t);
