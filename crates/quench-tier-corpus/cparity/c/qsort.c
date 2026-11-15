#include <stdio.h>
#include <stdint.h>
void qs(int32_t*a,int lo,int hi){while(lo<hi){int32_t p=a[(lo+hi)>>1];int i=lo,j=hi;while(i<=j){while(a[i]<p)i++;while(a[j]>p)j--;if(i<=j){int32_t t=a[i];a[i]=a[j];a[j]=t;i++;j--;}}
if(j-lo<hi-i){qs(a,lo,j);lo=i;}else{qs(a,i,hi);hi=j;}}}
int main(){enum{n=200000};static int32_t a[n];int32_t h=0;for(int r=0;r<10;r++){int32_t x=12345+r;for(int i=0;i<n;i++){x=(int32_t)((uint32_t)x*1103515245u+12345u);a[i]=x;}qs(a,0,n-1);h=(int32_t)((uint32_t)h*31u+(uint32_t)a[n>>1]+(uint32_t)a[0]+(uint32_t)a[n-1]);}printf("%d\n",h);}
