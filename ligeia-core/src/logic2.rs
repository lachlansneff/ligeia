use std::marker::PhantomData;

use itertools::Itertools;

use crate::{waves2::Timesteps, forest::{DataIndex, EventStorage, Event}};

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct DataWidth(pub u32);

pub trait Logic: Copy + 'static {
    /// For example, "0b".
    const FORMAT_PREFIX: &'static str;

    /// Used because returning `impl Iterator` from a trait method is not allowed yet.
    type UnpackIter<'a>: Iterator<Item = Self> where Self: 'a;

    fn bytes(width: DataWidth) -> usize;

    fn pack(logics: impl Iterator<Item = Self>, output: &mut [u8]);
    fn unpack<'a>(width: DataWidth, data: &'a [u8]) -> Self::UnpackIter<'a>;
}

pub trait Combine<L: Logic> {
    fn combine(storage: &mut EventStorage<L>, lhs: &Event<L>, rhs: &Event<L>) -> Event<L>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Two {
    Zero = 0,
    One = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Four {
    Zero = 0,
    One = 1,
    Unknown = 2,
    HighImpedance = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nine {
    ZeroStrong = 0,
    OneStrong = 1,
    ZeroWeak = 2,
    OneWeak = 3,
    UnknownStrong = 4,
    UnknownWeak = 5,
    ZeroUnknown = 6,
    OneUnknown = 7,
    HighImpedance = 8,
}

impl Logic for Two {
    const FORMAT_PREFIX: &'static str = "0b";

    type UnpackIter<'a> where Self: 'a = impl Iterator<Item = Self>;

    fn bytes(width: DataWidth) -> usize {
        (width.0 as usize + 8 - 1) / 8
    }

    fn pack(logics: impl Iterator<Item = Self>, output: &mut [u8]) {
        let mut i = 0;
        for chunk in &logics.chunks(8) {
            let mut byte = 0;
            for (i, logic) in chunk.enumerate() {
                byte |= (logic as u8) << i;
            }
            output[i] = byte;
            i += 1;
        }
    }

    fn unpack<'a>(width: DataWidth, data: &'a [u8]) -> Self::UnpackIter<'a> {
        data.iter().map(|&byte| {
            (0..8).map(move |i| {
                let mask = 0b1;
                match (byte >> i) & mask {
                    0 => Two::Zero,
                    1 => Two::One,
                    _ => unreachable!(),
                }
            })
        }).flatten().take(width.0 as usize)
    }
}

impl Logic for Four {
    const FORMAT_PREFIX: &'static str = "0q";

    type UnpackIter<'a> where Self: 'a = impl Iterator<Item = Self>;

    fn bytes(width: DataWidth) -> usize {
        (width.0 as usize + 4 - 1) / 4
    }

    fn pack(logics: impl Iterator<Item = Self>, output: &mut [u8]) {
        let mut i = 0;
        for chunk in &logics.chunks(4) {
            let mut byte = 0;
            for (i, logic) in chunk.enumerate() {
                byte |= (logic as u8) << (i * 2);
            }
            output[i] = byte;
            i += 1;
        }
    }

    fn unpack<'a>(width: DataWidth, data: &'a [u8]) -> Self::UnpackIter<'a> {
        data.iter().map(|&byte| {
            (0..8).step_by(2).map(move |i| {
                let mask = 0b11;
                match (byte >> i) & mask {
                    0 => Four::Zero,
                    1 => Four::One,
                    2 => Four::Unknown,
                    3 => Four::HighImpedance,
                    _ => unreachable!(),
                }
            })
        }).flatten().take(width.0 as usize)
    }
}

impl Logic for Nine {
    const FORMAT_PREFIX: &'static str = "0z";

    type UnpackIter<'a> where Self: 'a = impl Iterator<Item = Self>;

    fn bytes(width: DataWidth) -> usize {
        (width.0 as usize + 2 - 1) / 2
    }

    fn pack(logics: impl Iterator<Item = Self>, output: &mut [u8]) {
        let mut i = 0;
        for chunk in &logics.chunks(2) {
            let mut byte = 0;
            for (i, logic) in chunk.enumerate() {
                byte |= (logic as u8) << (i * 4);
            }
            output[i] = byte;
            i += 1;
        }
    }

    fn unpack<'a>(width: DataWidth, data: &'a [u8]) -> Self::UnpackIter<'a> {
        data.iter().map(|&byte| {
            (0..8).step_by(4).map(move |i| {
                let mask = 0b1111;
                match (byte >> i) & mask {
                    0 => Nine::ZeroStrong,
                    1 => Nine::OneStrong,
                    2 => Nine::ZeroWeak,
                    3 => Nine::OneWeak,
                    4 => Nine::UnknownStrong,
                    5 => Nine::UnknownWeak,
                    6 => Nine::ZeroUnknown,
                    7 => Nine::OneUnknown,
                    8 => Nine::HighImpedance,
                    _ => unreachable!(),
                }
            })
        }).flatten().take(width.0 as usize)
    }
}
