//! i64 Native kernels as a dispatch table.

use crate::unwind::Trap;

macro_rules! bin_i64 {
    ($($name:ident => $apply:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub enum BinI64 {
            $($name),+
        }

        impl BinI64 {
            pub fn is_rel(self) -> bool {
                (self as u8) >= BinI64::Eq as u8
            }

            pub fn apply(self, lhs: i64, rhs: i64) -> Result<i64, Trap> {
                const TABLE: &[fn(i64, i64) -> Result<i64, Trap>] = &[$($apply),+];
                TABLE[self as usize](lhs, rhs)
            }
        }
    };
}

macro_rules! un_i64 {
    ($($name:ident => $apply:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub enum UnI64 {
            $($name),+
        }

        impl UnI64 {
            pub fn apply(self, src: i64) -> i64 {
                const TABLE: &[fn(i64) -> i64] = &[$($apply),+];
                TABLE[self as usize](src)
            }
        }
    };
}

bin_i64! {
    Add => add,
    Sub => sub,
    Mul => mul,
    DivS => div_s,
    DivU => div_u,
    RemS => rem_s,
    RemU => rem_u,
    And => and,
    Or => or,
    Xor => xor,
    Shl => shl,
    ShrS => shr_s,
    ShrU => shr_u,
    Rotl => rotl,
    Rotr => rotr,
    Eq => eq,
    Ne => ne,
    LtS => lt_s,
    LtU => lt_u,
    LeS => le_s,
    LeU => le_u,
    GtS => gt_s,
    GtU => gt_u,
    GeS => ge_s,
    GeU => ge_u,
}

un_i64! {
    Clz => clz,
    Ctz => ctz,
    Popcnt => popcnt,
    Eqz => eqz,
    Extend8S => extend8_s,
    Extend16S => extend16_s,
    Extend32S => extend32_s,
}

fn add(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs.wrapping_add(rhs))
}
fn sub(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs.wrapping_sub(rhs))
}
fn mul(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs.wrapping_mul(rhs))
}
fn div_s(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    if lhs == i64::MIN && rhs == -1 {
        return Err(Trap::IntegerOverflow);
    }
    Ok(lhs / rhs)
}
fn div_u(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    Ok(((lhs as u64) / (rhs as u64)) as i64)
}
fn rem_s(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    Ok(lhs.wrapping_rem(rhs))
}
fn rem_u(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    Ok(((lhs as u64) % (rhs as u64)) as i64)
}
fn and(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs & rhs)
}
fn or(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs | rhs)
}
fn xor(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs ^ rhs)
}
fn shift_mask(rhs: i64) -> u32 {
    (rhs as u32) & 63
}
fn shl(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs.wrapping_shl(shift_mask(rhs)))
}
fn shr_s(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs.wrapping_shr(shift_mask(rhs)))
}
fn shr_u(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(((lhs as u64).wrapping_shr(shift_mask(rhs))) as i64)
}
fn rotl(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs.rotate_left(shift_mask(rhs)))
}
fn rotr(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    Ok(lhs.rotate_right(shift_mask(rhs)))
}
fn flag(cond: bool) -> Result<i64, Trap> {
    Ok(i64::from(cond))
}
fn eq(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag(lhs == rhs)
}
fn ne(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag(lhs != rhs)
}
fn lt_s(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag(lhs < rhs)
}
fn lt_u(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag((lhs as u64) < (rhs as u64))
}
fn le_s(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag(lhs <= rhs)
}
fn le_u(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag((lhs as u64) <= (rhs as u64))
}
fn gt_s(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag(lhs > rhs)
}
fn gt_u(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag((lhs as u64) > (rhs as u64))
}
fn ge_s(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag(lhs >= rhs)
}
fn ge_u(lhs: i64, rhs: i64) -> Result<i64, Trap> {
    flag((lhs as u64) >= (rhs as u64))
}
fn clz(src: i64) -> i64 {
    src.leading_zeros() as i64
}
fn ctz(src: i64) -> i64 {
    src.trailing_zeros() as i64
}
fn popcnt(src: i64) -> i64 {
    src.count_ones() as i64
}
fn eqz(src: i64) -> i64 {
    i64::from(src == 0)
}
fn extend8_s(src: i64) -> i64 {
    src as i8 as i64
}
fn extend16_s(src: i64) -> i64 {
    src as i16 as i64
}
fn extend32_s(src: i64) -> i64 {
    src as i32 as i64
}
