use std::{convert::TryFrom, marker::PhantomData, mem, ops::Deref, ptr::{self, NonNull}};

use crate::waves::ChangeHeader;


pub unsafe trait Logic {
    /// Number of units per byte.
    const PER_BYTE: usize;
    const FORMAT_PREFIX: &'static str;

    unsafe fn get_unit(b: *const u8, offset: usize) -> Self;
    unsafe fn set_unit(b: *mut u8, offset: usize, logic: Self);
}

pub trait Combine<L: Logic> {
    fn combine(lhs: LogicSliceMut<L>, rhs: LogicSlice<L>);
}

struct LogicSliceInner<L: Logic> {
    header: ChangeHeader,
    ptr: NonNull<u8>,
    width: usize,
    _marker: PhantomData<L>,
}

#[repr(transparent)]
pub struct LogicSlice<'a, L: Logic> {
    inner: LogicSliceInner<L>,
    _marker: PhantomData<&'a [u8]>,
}

#[repr(transparent)]
pub struct LogicSliceMut<'a, L: Logic> {
    inner: LogicSliceInner<L>,
    _marker: PhantomData<&'a mut [u8]>,
}

pub struct LogicArray<L: Logic> {
    inner: LogicSliceInner<L>,
}

impl<'a, L: Logic> LogicSlice<'a, L> {
    pub(crate) fn new(header: ChangeHeader, width: usize, ptr: NonNull<u8>) -> Self {
        Self {
            inner: LogicSliceInner {
                header,
                ptr,
                width,
                _marker: PhantomData,
            },
            _marker: PhantomData,
        }
    }

    pub fn get(&self, offset: usize) -> L {
        assert!(offset < self.inner.width, "attempted to get a logical unit out of bounds");
        unsafe {
            L::get_unit(self.inner.ptr.as_ptr(), offset)
        }
    }

    pub fn iter(&self) -> LogicIter<L> {
        LogicIter {
            ptr: self.inner.ptr.as_ptr(),
            width: self.inner.width,
            offset: 0,
            _marker: PhantomData,
        }
    }
}

impl<'a, L: Logic> LogicSliceMut<'a, L> {
    pub(crate) fn new(header: ChangeHeader, width: usize, ptr: NonNull<u8>) -> Self {
        Self {
            inner: LogicSliceInner {
                header,
                ptr,
                width,
                _marker: PhantomData,
            },
            _marker: PhantomData,
        }
    }

    pub fn set(&mut self, offset: usize, logic: L) {
        assert!(offset < self.inner.width, "attempted to set a logical unit out of bounds");
        unsafe {
            L::set_unit(self.inner.ptr.as_ptr(), offset, logic)
        }
    }
}

impl<'a, L: Logic> Deref for LogicSliceMut<'a, L> {
    type Target = LogicSlice<'a, L>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `LogicSliceMut` and `LogicSlice` have exactly the same representation.
        unsafe {
            &*(self as *const Self as *const LogicSlice<'a, L>)
        }
    }
}

impl<L: Logic> LogicArray<L> {
    pub fn new(header: ChangeHeader, width: usize) -> Self {
        let bytes = (width + L::PER_BYTE - 1) / L::PER_BYTE;
        let slice = Box::leak(vec![0u8; bytes].into_boxed_slice());
        let ptr = unsafe { NonNull::new_unchecked(slice.as_mut_ptr()) };

        Self {
            inner: LogicSliceInner {
                header,
                ptr,
                width,
                _marker: PhantomData,
            },
        }
    }

    pub fn get(&self, offset: usize) -> L {
        assert!(offset < self.inner.width, "attempted to get a logical unit out of bounds");
        unsafe {
            L::get_unit(self.inner.ptr.as_ptr(), offset)
        }
    }

    pub fn set(&mut self, offset: usize, logic: L) {
        assert!(offset < self.inner.width, "attempted to set a logical unit out of bounds");
        unsafe {
            L::set_unit(self.inner.ptr.as_ptr(), offset, logic)
        }
    }

    pub fn as_slice(&self) -> LogicSlice<L> {
        unsafe {
            mem::transmute(ptr::read(&self.inner))
        }
    }

    pub fn as_slice_mut(&mut self) -> LogicSliceMut<L> {
        unsafe {
            mem::transmute(ptr::read(&self.inner))
        }
    }

    pub fn iter(&self) -> LogicIter<L> {
        LogicIter {
            ptr: self.inner.ptr.as_ptr(),
            width: self.inner.width,
            offset: 0,
            _marker: PhantomData,
        }
    }
}

pub struct LogicIter<'a, L: Logic> {
    ptr: *const u8,
    width: usize,
    offset: usize,
    _marker: PhantomData<(&'a [u8], L)>,
}

impl<L: Logic> Iterator for LogicIter<'_, L> {
    type Item = L;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset < self.width {
            let unit = unsafe { L::get_unit(self.ptr, self.offset) };
            self.offset += 1;
            Some(unit)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TwoLogic {
    Zero = 0,
    One = 1,
}

unsafe impl Logic for TwoLogic {
    const PER_BYTE: usize = 8;

    const FORMAT_PREFIX: &'static str = "0b";

    unsafe fn get_unit(b: *const u8, offset: usize) -> Self {
        let offset_bytes = offset / Self::PER_BYTE;
        let offset_bits = offset % Self::PER_BYTE;
        unsafe {
            mem::transmute(b.add(offset_bytes).read() & (1 << offset_bits) >> offset_bits)
        }
    }

    unsafe fn set_unit(b: *mut u8, offset: usize, u: Self) {
        let offset_bytes = offset / Self::PER_BYTE;
        let offset_bits = offset % Self::PER_BYTE;
        unsafe {
            let c = b.add(offset_bytes);
            c.write((c.read() & !(1 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FourLogic {
    Zero = 0,
    One = 1,
    Unknown = 2,
    HighImpedance = 3,
}

unsafe impl Logic for FourLogic {
    const PER_BYTE: usize = 4;

    const FORMAT_PREFIX: &'static str = "0q";

    unsafe fn get_unit(b: *const u8, offset: usize) -> Self {
        let offset_byte = offset / Self::PER_BYTE;
        let offset_bits = offset % Self::PER_BYTE;
        unsafe {
            mem::transmute(b.add(offset_byte).read() & (0b11 << offset_bits) >> offset_bits)
        }
    }

    unsafe fn set_unit(b: *mut u8, offset: usize, u: Self) {
        let offset_byte = offset / Self::PER_BYTE;
        let offset_bits = offset % Self::PER_BYTE;
        unsafe {
            let c = b.add(offset_byte);
            c.write((c.read() & !(0b11 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

unsafe impl Logic for NineLogic {
    const PER_BYTE: usize = 2;
    const FORMAT_PREFIX: &'static str = "0n";

    unsafe fn get_unit(b: *const u8, offset: usize) -> Self {
        let offset_bytes = offset / Self::PER_BYTE;
        let offset_bits = offset % Self::PER_BYTE;
        unsafe {
            mem::transmute((b.add(offset_bytes).read() & (0b1111 << offset_bits) >> offset_bits).clamp(0, Self::HighImpedance as u8))
        }
    }

    unsafe fn set_unit(b: *mut u8, offset: usize, u: Self) {
        let offset_bytes = offset / Self::PER_BYTE;
        let offset_bits = offset % Self::PER_BYTE;
        unsafe {
            let c = b.add(offset_bytes);
            c.write((c.read() & !(0b1111 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logic_array_set_get() {
        let mut array = LogicArray::<NineLogic>::new(ChangeHeader {
            ts: 0,
        }, 1);

        array.set(0, NineLogic::UnknownStrong);

        assert_eq!(array.get(0), NineLogic::UnknownStrong);
    }

    #[test]
    fn logic_array_set_iter() {
        let mut array = LogicArray::<NineLogic>::new(ChangeHeader {
            ts: 0,
        }, 1);
        array.set(0, NineLogic::OneWeak);
        
        let mut iter = array.iter();
        assert_eq!(iter.next(), Some(NineLogic::OneWeak));
        assert_eq!(iter.next(), None);
    }
}