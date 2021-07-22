#![feature(
    const_generics, const_evaluatable_checked,
    allocator_api,
    new_uninit,
)]

#![warn(unsafe_op_in_unsafe_fn)]

use std::cmp;


mod implicit_forest;
mod array;

pub trait Merge<U: Unit> {
    type Header: AsRef<[U::Chunk]>;

    fn merge(lhs: Self::Header, rhs: Self::Header) -> Self::Header;
}

pub trait Mix<U: Unit> {
    /// Mix/aggregate two chunks together. `count` is the number of units (lsb) in the chunks.
    fn mix_partial(lhs: U::Chunk, rhs: U::Chunk, count: usize) -> U::Chunk;

    fn mix(lhs: U::Chunk, rhs: U::Chunk) -> U::Chunk {
        Self::mix_partial(lhs, rhs, U::PER_CHUNK)
    }
}

pub trait Unit 
where
    Self: Sized
{
    type Chunk: Copy;
    const EMPTY: Self::Chunk;
    /// Number of units for chunk.
    const PER_CHUNK: usize;
    const FORMAT_PREFIX: &'static str;

    fn from_chunk(c: Self::Chunk) -> [Self; Self::PER_CHUNK];
    fn into_chunk(this: [Self; Self::PER_CHUNK]) -> Self::Chunk;
    /// `count` is the number of units to keep lsb.
    fn mask_off_extra(c: Self::Chunk, count: usize) -> Self::Chunk;
}

#[derive(Debug, Clone, Copy)]
pub enum Bit {
    Zero = 0,
    One = 1,
}

impl Unit for Bit {
    type Chunk = u8;

    const EMPTY: Self::Chunk = 0;

    const PER_CHUNK: usize = 8;

    const FORMAT_PREFIX: &'static str = "0b";

    fn from_chunk(c: Self::Chunk) -> [Self; Self::PER_CHUNK] {
        fn to(u: u8) -> Bit {
            match u {
                0 => Bit::Zero,
                1 => Bit::One,
                _ => if cfg!(debug_assertions) {
                    unreachable!()
                } else {
                    unsafe { std::hint::unreachable_unchecked() }
                }
            }
        }

        [
            to((c & 0b0000_0001) >> 0),
            to((c & 0b0000_0010) >> 1),
            to((c & 0b0000_0100) >> 2),
            to((c & 0b0000_1000) >> 3),
            to((c & 0b0001_0000) >> 4),
            to((c & 0b0010_0000) >> 5),
            to((c & 0b0100_0000) >> 6),
            to((c & 0b1000_0000) >> 7),
        ]
    }

    fn into_chunk([a, b, c, d, e, f, g, h]: [Self; Self::PER_CHUNK]) -> Self::Chunk {
        a as u8
        | (b as u8) << 1
        | (c as u8) << 2
        | (d as u8) << 3
        | (e as u8) << 4
        | (f as u8) << 5
        | (g as u8) << 6
        | (h as u8) << 7
    }

    fn mask_off_extra(c: Self::Chunk, count: usize) -> Self::Chunk {
        assert!(count <= 7);
        c & (u8::MAX >> (8 - count))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Qit {
    Zero = 0,
    One = 1,
    X = 2,
    Z = 3,
}

impl Unit for Qit {
    type Chunk = u8;

    const EMPTY: Self::Chunk = 0;

    const PER_CHUNK: usize = 4;

    const FORMAT_PREFIX: &'static str = "0q";

    fn from_chunk(c: Self::Chunk) -> [Self; Self::PER_CHUNK] {
        fn to(u: u8) -> Qit {
            match u {
                0 => Qit::Zero,
                1 => Qit::One,
                2 => Qit::X,
                3 => Qit::Z,
                _ => if cfg!(debug_assertions) {
                    unreachable!()
                } else {
                    unsafe { std::hint::unreachable_unchecked() }
                }
            }
        }

        [
            to((c & 0b0000_0011) >> 0),
            to((c & 0b0000_1100) >> 2),
            to((c & 0b0011_0000) >> 4),
            to((c & 0b1100_0000) >> 6),
        ]
    }

    fn into_chunk([a, b, c, d]: [Self; Self::PER_CHUNK]) -> Self::Chunk {
        a as u8
        | (b as u8) << 2
        | (c as u8) << 4
        | (d as u8) << 6
    }

    fn mask_off_extra(c: Self::Chunk, count: usize) -> Self::Chunk {
        assert!(count <= 3);
        c & (u8::MAX >> (8 - (count * 2)))
    }
}

pub struct Max;

impl Mix<Bit> for Max {
    fn mix_partial(lhs: u8, rhs: u8, count: usize) -> u8 {
        assert!(count <= 8);
        cmp::max(lhs, rhs)
    }
}

impl Mix<Qit> for Max {
    fn mix_partial(lhs: u8, rhs: u8, count: usize) -> u8 {
        assert!(count <= 4);
        cmp::max(lhs, rhs)
    }
}
