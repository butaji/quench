//! i32 Native kernels as a dispatch table, not a branch nest.

use crate::unwind::Trap;

macro_rules! bin_i32 {
    ($($name:ident => $apply:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub enum BinI32 {
            $($name),+
        }

        impl BinI32 {
            pub fn apply(self, lhs: i32, rhs: i32) -> Result<i32, Trap> {
                const TABLE: &[fn(i32, i32) -> Result<i32, Trap>] = &[$($apply),+];
                TABLE[self as usize](lhs, rhs)
            }
        }
    };
}

macro_rules! un_i32 {
    ($($name:ident => $apply:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub enum UnI32 {
            $($name),+
        }

        impl UnI32 {
            pub fn apply(self, src: i32) -> i32 {
                const TABLE: &[fn(i32) -> i32] = &[$($apply),+];
                TABLE[self as usize](src)
            }
        }
    };
}

bin_i32! {
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

un_i32! {
    Clz => clz,
    Ctz => ctz,
    Popcnt => popcnt,
    Eqz => eqz,
    Extend8S => extend8_s,
    Extend16S => extend16_s,
}

fn add(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs.wrapping_add(rhs))
}

fn sub(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs.wrapping_sub(rhs))
}

fn mul(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs.wrapping_mul(rhs))
}

fn div_s(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    if lhs == i32::MIN && rhs == -1 {
        return Err(Trap::IntegerOverflow);
    }
    Ok(lhs / rhs)
}

fn div_u(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    Ok(((lhs as u32) / (rhs as u32)) as i32)
}

fn rem_s(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    Ok(lhs.wrapping_rem(rhs))
}

fn rem_u(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    if rhs == 0 {
        return Err(Trap::IntegerDivideByZero);
    }
    Ok(((lhs as u32) % (rhs as u32)) as i32)
}

fn and(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs & rhs)
}

fn or(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs | rhs)
}

fn xor(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs ^ rhs)
}

fn shift_mask(rhs: i32) -> u32 {
    (rhs as u32) & 31
}

fn shl(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs.wrapping_shl(shift_mask(rhs)))
}

fn shr_s(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs.wrapping_shr(shift_mask(rhs)))
}

fn shr_u(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(((lhs as u32).wrapping_shr(shift_mask(rhs))) as i32)
}

fn rotl(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs.rotate_left(shift_mask(rhs)))
}

fn rotr(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    Ok(lhs.rotate_right(shift_mask(rhs)))
}

fn flag(cond: bool) -> Result<i32, Trap> {
    Ok(i32::from(cond))
}

fn eq(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag(lhs == rhs)
}

fn ne(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag(lhs != rhs)
}

fn lt_s(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag(lhs < rhs)
}

fn lt_u(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag((lhs as u32) < (rhs as u32))
}

fn le_s(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag(lhs <= rhs)
}

fn le_u(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag((lhs as u32) <= (rhs as u32))
}

fn gt_s(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag(lhs > rhs)
}

fn gt_u(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag((lhs as u32) > (rhs as u32))
}

fn ge_s(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag(lhs >= rhs)
}

fn ge_u(lhs: i32, rhs: i32) -> Result<i32, Trap> {
    flag((lhs as u32) >= (rhs as u32))
}

fn clz(src: i32) -> i32 {
    src.leading_zeros() as i32
}

fn ctz(src: i32) -> i32 {
    src.trailing_zeros() as i32
}

fn popcnt(src: i32) -> i32 {
    src.count_ones() as i32
}

fn eqz(src: i32) -> i32 {
    i32::from(src == 0)
}

fn extend8_s(src: i32) -> i32 {
    src as i8 as i32
}

fn extend16_s(src: i32) -> i32 {
    src as i16 as i32
}

#[cfg(test)]
mod tests {
    use super::{BinI32, UnI32};
    use crate::unwind::Trap;

    #[test]
    fn add_wraps() {
        assert_eq!(BinI32::Add.apply(i32::MAX, 1), Ok(i32::MIN));
    }

    #[test]
    fn div_s_traps() {
        assert_eq!(BinI32::DivS.apply(1, 0), Err(Trap::IntegerDivideByZero));
        assert_eq!(BinI32::DivS.apply(i32::MIN, -1), Err(Trap::IntegerOverflow));
    }

    #[test]
    fn rem_s_min_neg_one_is_zero() {
        assert_eq!(BinI32::RemS.apply(i32::MIN, -1), Ok(0));
    }

    #[test]
    fn unary_bits() {
        assert_eq!(UnI32::Clz.apply(0), 32);
        assert_eq!(UnI32::Eqz.apply(0), 1);
        assert_eq!(UnI32::Extend8S.apply(0x0000_00ff), -1);
    }
}
