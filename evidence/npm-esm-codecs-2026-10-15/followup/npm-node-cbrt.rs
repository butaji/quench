// Adapted from V8 src/base/ieee754.cc at Node 26.12's pinned source.
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this software is freely
// granted, provided that this notice is preserved.
// Modified significantly by Google Inc.; Copyright 2016 the V8 project authors.
pub fn cbrt(x:f64)->f64 {
    const NORMAL_BIAS:u32=715094163;
    const SUBNORMAL_BIAS:u32=696219795;
    const COEFFICIENTS:[f64;5]=[
        f64::from_bits(0x3ffe03e60f61e692),f64::from_bits(0xbffe28e092f02420),
        f64::from_bits(0x3ff9f1604a49d6c2),f64::from_bits(0xbfe844cbbee751d9),
        f64::from_bits(0x3fc2b000d4e4edd7)];
    const ESTIMATE_SIGNIFICAND_BITS:u32=23;
    const DISCARDED_BITS:u32=f64::MANTISSA_DIGITS-ESTIMATE_SIGNIFICAND_BITS;
    const ROUND_AWAY_BIAS:u64=1_u64<<(DISCARDED_BITS+1);
    const ROUND_AWAY_MASK:u64=u64::MAX<<DISCARDED_BITS;
    let bits=x.to_bits();
    let high=(bits>>u32::BITS)as u32;
    let sign=high&((-0.0_f64).to_bits()>>u32::BITS)as u32;
    let magnitude=high^sign;
    if !x.is_finite(){return x+x;}
    let rough=if x.abs()<f64::MIN_POSITIVE {
        if x==0.0{return x;}
        let scaled=x*((1_u64<<(f64::MANTISSA_DIGITS+1))as f64);
        let high=(scaled.abs().to_bits()>>u32::BITS)as u32;
        high/3+SUBNORMAL_BIAS
    }else{magnitude/3+NORMAL_BIAS};
    let mut t=f64::from_bits(u64::from(sign|rough)<<u32::BITS);
    let r=(t*t)*(t/x);
    let [p0,p1,p2,p3,p4]=COEFFICIENTS;
    t=t*((p0+r*(p1+r*p2))+((r*r)*r)*(p3+r*p4));
    t=f64::from_bits((t.to_bits()+ROUND_AWAY_BIAS)&ROUND_AWAY_MASK);
    let s=t*t;
    let r=x/s;
    let w=t+t;
    let r=(r-t)/(w+r);
    t+t*r
}

pub fn atan2(y:f64,x:f64)->f64 {
    const PI_LO:f64=f64::from_bits(0x3ca1a62633145c07);
    const LARGE_RATIO_EXPONENT:i32=60;
    const HIGH_WORD_FRACTION_BITS:u32=f64::MANTISSA_DIGITS-1-u32::BITS;
    use std::f64::consts::{PI,FRAC_PI_2,FRAC_PI_4};
    if x.is_nan()||y.is_nan(){return x+y;}
    if x==1.0{return libm::atan(y);}
    if y==0.0{return if x.is_sign_negative(){PI.copysign(y)}else{y};}
    if x==0.0{return FRAC_PI_2.copysign(y);}
    if x.is_infinite(){
        let magnitude=if y.is_infinite(){if x.is_sign_negative(){3.0*FRAC_PI_4}else{FRAC_PI_4}}
            else if x.is_sign_negative(){PI}else{0.0};
        return magnitude.copysign(y);
    }
    if y.is_infinite(){return FRAC_PI_2.copysign(y);}
    let x_high=(x.abs().to_bits()>>u32::BITS)as i32;
    let y_high=(y.abs().to_bits()>>u32::BITS)as i32;
    let ratio_exponent=(y_high-x_high)>>HIGH_WORD_FRACTION_BITS;
    if ratio_exponent>LARGE_RATIO_EXPONENT{return FRAC_PI_2.copysign(y);}
    let z=if x.is_sign_negative()&&ratio_exponent< -LARGE_RATIO_EXPONENT {0.0}else{libm::atan((y/x).abs())};
    if !x.is_sign_negative(){z.copysign(y)}
    else if y.is_sign_negative(){(z-PI_LO)-PI}
    else{PI-(z-PI_LO)}
}
