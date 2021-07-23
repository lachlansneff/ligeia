
mod slice;
mod array;

use std::convert::TryFrom;
use std::mem;

pub use self::slice::{UnitSlice, UnitIter, Mut, Immut};
pub use  self::array::UnitArray;

pub trait Header<U: Unit>: Copy + AsRef<[U::Chunk]>
{
    const CHUNKS: usize;
    const EMPTY: Self;
    fn from_chunks(chunks: &[U::Chunk]) -> Self;
    fn aggregate(lhs: UnitSlice<U, Self, Mut>, rhs: UnitSlice<U, Self>);
}

pub unsafe trait Unit: Sized {
    type Chunk: Copy;
    const EMPTY: Self::Chunk;
    /// Number of units for chunk.
    const PER_CHUNK: usize;
    const FORMAT_PREFIX: &'static str;

    unsafe fn get_unit(chunks: *const Self::Chunk, offset: usize) -> Self;
    unsafe fn set_unit(chunks: *mut Self::Chunk, offset: usize, u: Self);
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum TwoLogic {
    Zero = 0,
    One = 1,
}

unsafe impl Unit for TwoLogic {
    type Chunk = u8;

    const EMPTY: Self::Chunk = 0;

    const PER_CHUNK: usize = 8;

    const FORMAT_PREFIX: &'static str = "0b";

    // fn from_chunk(c: Self::Chunk) -> [Self; Self::PER_CHUNK] {
    //     fn to(u: u8) -> TwoLogic {
    //         let u  = u & 0b1;
    //         unsafe { mem::transmute(u) }
    //     }

    //     [
    //         to((c & 0b0000_0001) >> 0),
    //         to((c & 0b0000_0010) >> 1),
    //         to((c & 0b0000_0100) >> 2),
    //         to((c & 0b0000_1000) >> 3),
    //         to((c & 0b0001_0000) >> 4),
    //         to((c & 0b0010_0000) >> 5),
    //         to((c & 0b0100_0000) >> 6),
    //         to((c & 0b1000_0000) >> 7),
    //     ]
    // }

    // fn into_chunk([a, b, c, d, e, f, g, h]: [Self; Self::PER_CHUNK]) -> Self::Chunk {
    //     a as u8
    //     | (b as u8) << 1
    //     | (c as u8) << 2
    //     | (d as u8) << 3
    //     | (e as u8) << 4
    //     | (f as u8) << 5
    //     | (g as u8) << 6
    //     | (h as u8) << 7
    // }

    unsafe fn get_unit(chunks: *const Self::Chunk, offset: usize) -> Self {
        let offset_chunks = offset / Self::PER_CHUNK;
        let offset_bits = offset % Self::PER_CHUNK;
        unsafe {
            mem::transmute(chunks.add(offset_chunks).read() & (1 << offset_bits) >> offset_bits)
        }
    }

    unsafe fn set_unit(chunks: *mut Self::Chunk, offset: usize, u: Self) {
        let offset_chunks = offset / Self::PER_CHUNK;
        let offset_bits = offset % Self::PER_CHUNK;
        unsafe {
            let c = chunks.add(offset_chunks);
            c.write((c.read() & !(1 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum FourLogic {
    Zero = 0,
    One = 1,
    Unknown = 2,
    HighImpedance = 3,
}

unsafe impl Unit for FourLogic {
    type Chunk = u8;

    const EMPTY: Self::Chunk = 0;

    const PER_CHUNK: usize = 4;

    const FORMAT_PREFIX: &'static str = "0q";

    unsafe fn get_unit(chunks: *const Self::Chunk, offset: usize) -> Self {
        let offset_chunks = offset / Self::PER_CHUNK;
        let offset_bits = offset % Self::PER_CHUNK;
        unsafe {
            mem::transmute(chunks.add(offset_chunks).read() & (0b11 << offset_bits) >> offset_bits)
        }
    }

    unsafe fn set_unit(chunks: *mut Self::Chunk, offset: usize, u: Self) {
        let offset_chunks = offset / Self::PER_CHUNK;
        let offset_bits = offset % Self::PER_CHUNK;
        unsafe {
            let c = chunks.add(offset_chunks);
            c.write((c.read() & !(0b11 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }

    // fn from_chunk(c: Self::Chunk) -> [Self; Self::PER_CHUNK] {
    //     fn to(u: u8) -> FourLogic {
    //         let u = u & 0b11;
    //         unsafe { mem::transmute(u) }
    //     }

    //     [
    //         to((c & 0b0000_0011) >> 0),
    //         to((c & 0b0000_1100) >> 2),
    //         to((c & 0b0011_0000) >> 4),
    //         to((c & 0b1100_0000) >> 6),
    //     ]
    // }

    // fn into_chunk([a, b, c, d]: [Self; Self::PER_CHUNK]) -> Self::Chunk {
    //     a as u8
    //     | (b as u8) << 2
    //     | (c as u8) << 4
    //     | (d as u8) << 6
    // }
}

pub struct LogicConversionFailed;

impl TryFrom<FourLogic> for TwoLogic {
    type Error = LogicConversionFailed;

    fn try_from(value: FourLogic) -> Result<Self, Self::Error> {
        match value {
            FourLogic::Zero => Ok(TwoLogic::Zero),
            FourLogic::One => Ok(TwoLogic::One),
            _ => Err(LogicConversionFailed)
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum NineLogic {
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

unsafe impl Unit for NineLogic {
    type Chunk = u8;

    const EMPTY: Self::Chunk = 0;

    const PER_CHUNK: usize = 2;

    const FORMAT_PREFIX: &'static str = "0n";

    unsafe fn get_unit(chunks: *const Self::Chunk, offset: usize) -> Self {
        let offset_chunks = offset / Self::PER_CHUNK;
        let offset_bits = offset % Self::PER_CHUNK;
        unsafe {
            mem::transmute((chunks.add(offset_chunks).read() & (0b1111 << offset_bits) >> offset_bits).clamp(0, Self::HighImpedance as u8))
        }
    }

    unsafe fn set_unit(chunks: *mut Self::Chunk, offset: usize, u: Self) {
        let offset_chunks = offset / Self::PER_CHUNK;
        let offset_bits = offset % Self::PER_CHUNK;
        unsafe {
            let c = chunks.add(offset_chunks);
            c.write((c.read() & !(0b1111 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }

    // fn from_chunk(c: Self::Chunk) -> [Self; Self::PER_CHUNK] {
    //     fn to(u: u8) -> NineLogic {
    //         let u  = u.clamp(0, NineLogic::HighImpedance as u8);
    //         unsafe  { mem::transmute(u) }
    //     }

    //     [
    //         to((c & 0b0000_1111) >> 0),
    //         to((c & 0b1111_0000) >> 4),
    //     ]
    // }

    // fn into_chunk([a, b]: [Self; Self::PER_CHUNK]) -> Self::Chunk {
    //     a as u8
    //     | (b as u8) << 4
    // }
}

impl TryFrom<NineLogic> for TwoLogic {
    type Error = LogicConversionFailed;

    fn try_from(value: NineLogic) -> Result<Self, Self::Error> {
        match value {
            NineLogic::ZeroStrong | NineLogic::ZeroWeak => Ok(TwoLogic::Zero),
            NineLogic::OneStrong | NineLogic::OneWeak => Ok(TwoLogic::One),
            _ => Err(LogicConversionFailed)
        }
    }
}
