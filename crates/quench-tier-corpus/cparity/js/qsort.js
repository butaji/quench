function qs(a,lo,hi){while(lo<hi){const p=a[(lo+hi)>>1];let i=lo,j=hi;while(i<=j){while(a[i]<p)i++;while(a[j]>p)j--;if(i<=j){const t=a[i];a[i]=a[j];a[j]=t;i++;j--;}}
if(j-lo<hi-i){qs(a,lo,j);lo=i;}else{qs(a,i,hi);hi=j;}}}
const n=200000,a=new Int32Array(n);let h=0;
for(let r=0;r<10;r++){let x=12345+r|0;for(let i=0;i<n;i++){x=(Math.imul(x,1103515245)+12345)|0;a[i]=x;}qs(a,0,n-1);h=(h*31+a[n>>1]+a[0]+a[n-1])|0;}
console.log(h);
