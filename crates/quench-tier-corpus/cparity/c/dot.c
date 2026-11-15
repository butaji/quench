#include <stdio.h>
#include <stdlib.h>
double dot(double*a,double*b,int n,int reps){double s=0;for(int r=0;r<reps;r++){for(int i=0;i<n;i++)s+=a[i]*b[i];}return s;}
int main(){int n=32768;double*a=malloc(n*8),*b=malloc(n*8);for(int i=0;i<n;i++){a[i]=(i%100)*0.25;b[i]=(i%7)+1;}printf("%.17g\n",dot(a,b,n,600));}
