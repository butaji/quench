#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
/* mirrors JS semantics: ((c^b^x)&255)*2654435761 is a double product then ToInt32 in the xor */
int32_t crc(uint8_t*b,int n,int reps){int32_t c=-1,x=123456789;for(int r=0;r<reps;r++){for(int i=0;i<n;i++){x^=(int32_t)((uint32_t)x<<13);x^=(int32_t)((uint32_t)x>>17);x^=(int32_t)((uint32_t)x<<5);
 double p=(double)((c^b[i]^x)&255)*2654435761.0; /* < 2^40 exact */
 int32_t pi=(int32_t)(uint32_t)(uint64_t)p; c=(int32_t)((uint32_t)c>>8)^pi; }}return c;}
int main(){int n=65536;uint8_t*b=malloc(n);for(int i=0;i<n;i++)b[i]=(i*31+7)&255;printf("%d\n",crc(b,n,60));}
