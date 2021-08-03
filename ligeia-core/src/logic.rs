use std::{
    convert::TryFrom,
    marker::PhantomData,
    mem,
    ops::Deref,
    ptr::{self, NonNull},
};

use crate::waves::Timesteps;

pub unsafe trait Logic: Copy + Default + 'static {
    /// Number of units per byte.
    const PER_BYTE: usize;
    const FORMAT_PREFIX: &'static str;

    /// `0` must be a valid logical unit.
    unsafe fn get_unit(b: *const u8, offset: usize) -> Self;
    unsafe fn set_unit(b: *mut u8, offset: usize, logic: Self);
}

pub trait Combine<L: Logic> {
    fn combine(lhs: (Timesteps, LogicSliceMut<L>), rhs: (Timesteps, LogicSlice<L>));
}

struct LogicSliceInner<L: Logic> {
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
    pub(crate) unsafe fn new(width: usize, ptr: NonNull<u8>) -> Self {
        Self {
            inner: LogicSliceInner {
                ptr,
                width,
                _marker: PhantomData,
            },
            _marker: PhantomData,
        }
    }

    pub fn get(&self, offset: usize) -> L {
        assert!(
            offset < self.inner.width,
            "attempted to get a logical unit out of bounds"
        );
        unsafe { L::get_unit(self.inner.ptr.as_ptr(), offset) }
    }

    pub fn width(&self) -> usize {
        self.inner.width
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
    pub(crate) unsafe fn new(width: usize, ptr: NonNull<u8>) -> Self {
        Self {
            inner: LogicSliceInner {
                ptr,
                width,
                _marker: PhantomData,
            },
            _marker: PhantomData,
        }
    }

    pub fn set(&mut self, offset: usize, logic: L) {
        assert!(
            offset < self.inner.width,
            "attempted to set a logical unit out of bounds"
        );
        unsafe { L::set_unit(self.inner.ptr.as_ptr(), offset, logic) }
    }
}

impl<'a, L: Logic> Deref for LogicSliceMut<'a, L> {
    type Target = LogicSlice<'a, L>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `LogicSliceMut` and `LogicSlice` have exactly the same representation.
        unsafe { &*(self as *const Self as *const LogicSlice<'a, L>) }
    }
}

impl<L: Logic> LogicArray<L> {
    pub fn new(width: usize, fill: L) -> Self {
        let bytes = (width + L::PER_BYTE - 1) / L::PER_BYTE;
        let slice = Box::leak(vec![0u8; bytes].into_boxed_slice());
        for i in 0..width {
            unsafe {
                L::set_unit(slice.as_mut_ptr(), i, fill);
            }
        }
        let ptr = unsafe { NonNull::new_unchecked(slice.as_mut_ptr()) };

        Self {
            inner: LogicSliceInner {
                ptr,
                width,
                _marker: PhantomData,
            },
        }
    }

    pub fn width(&self) -> usize {
        self.inner.width
    }

    pub fn get(&self, offset: usize) -> L {
        assert!(
            offset < self.inner.width,
            "attempted to get a logical unit out of bounds"
        );
        unsafe { L::get_unit(self.inner.ptr.as_ptr(), offset) }
    }

    pub fn set(&mut self, offset: usize, logic: L) {
        assert!(
            offset < self.inner.width,
            "attempted to set a logical unit out of bounds"
        );
        unsafe { L::set_unit(self.inner.ptr.as_ptr(), offset, logic) }
    }

    pub fn as_slice(&self) -> LogicSlice<L> {
        unsafe { mem::transmute(ptr::read(&self.inner)) }
    }

    pub fn as_slice_mut(&mut self) -> LogicSliceMut<L> {
        unsafe { mem::transmute(ptr::read(&self.inner)) }
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
pub enum Two {
    Zero = 0,
    One = 1,
}

unsafe impl Logic for Two {
    const PER_BYTE: usize = 8;

    const FORMAT_PREFIX: &'static str = "0b";

    unsafe fn get_unit(b: *const u8, offset: usize) -> Self {
        let offset_bytes = offset / Self::PER_BYTE;
        let offset_bits = offset % Self::PER_BYTE;
        unsafe { mem::transmute((b.add(offset_bytes).read() >> offset_bits) & 1) }
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

impl Default for Two {
    fn default() -> Self {
        Self::Zero
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Four {
    Zero = 0,
    One = 1,
    Unknown = 2,
    HighImpedance = 3,
}

unsafe impl Logic for Four {
    const PER_BYTE: usize = 4;

    const FORMAT_PREFIX: &'static str = "0q";

    unsafe fn get_unit(b: *const u8, offset: usize) -> Self {
        let offset_byte = offset / Self::PER_BYTE;
        let offset_bits = (offset % Self::PER_BYTE) * 2;
        unsafe { mem::transmute((b.add(offset_byte).read() >> offset_bits) & 0b11) }
    }

    unsafe fn set_unit(b: *mut u8, offset: usize, u: Self) {
        let offset_byte = offset / Self::PER_BYTE;
        let offset_bits = (offset % Self::PER_BYTE) * 2;
        unsafe {
            let c = b.add(offset_byte);
            c.write((c.read() & !(0b11 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }
}

impl Default for Four {
    fn default() -> Self {
        Self::Zero
    }
}

pub struct LogicConversionFailed;

impl TryFrom<Four> for Two {
    type Error = LogicConversionFailed;

    fn try_from(value: Four) -> Result<Self, Self::Error> {
        match value {
            Four::Zero => Ok(Two::Zero),
            Four::One => Ok(Two::One),
            _ => Err(LogicConversionFailed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
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

unsafe impl Logic for Nine {
    const PER_BYTE: usize = 2;
    const FORMAT_PREFIX: &'static str = "0n";

    unsafe fn get_unit(b: *const u8, offset: usize) -> Self {
        let offset_bytes = offset / Self::PER_BYTE;
        let offset_bits = (offset % Self::PER_BYTE) * 4;
        unsafe { mem::transmute((b.add(offset_bytes).read() >> offset_bits) & 0b1111) }
    }

    unsafe fn set_unit(b: *mut u8, offset: usize, u: Self) {
        let offset_bytes = offset / Self::PER_BYTE;
        let offset_bits = (offset % Self::PER_BYTE) * 4;
        unsafe {
            let c = b.add(offset_bytes);
            c.write((c.read() & !(0b1111 << offset_bits)) | ((u as u8) << offset_bits));
        }
    }
}

impl Default for Nine {
    fn default() -> Self {
        Self::UnknownWeak
    }
}

impl TryFrom<Nine> for Two {
    type Error = LogicConversionFailed;

    fn try_from(value: Nine) -> Result<Self, Self::Error> {
        match value {
            Nine::ZeroStrong | Nine::ZeroWeak => Ok(Two::Zero),
            Nine::OneStrong | Nine::OneWeak => Ok(Two::One),
            _ => Err(LogicConversionFailed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logic_array_set_get() {
        let mut array = LogicArray::<Nine>::new(1, Nine::HighImpedance);

        array.set(0, Nine::UnknownStrong);

        assert_eq!(array.get(0), Nine::UnknownStrong);
    }

    #[test]
    fn logic_array_set_iter() {
        let mut array = LogicArray::<Two>::new(3, Two::Zero);
        array.set(0, Two::One);

        let mut iter = array.iter();
        println!("{:#08b}", unsafe { *iter.ptr });
        assert_eq!(iter.next(), Some(Two::One));
        assert_eq!(iter.next(), Some(Two::Zero));
        assert_eq!(iter.next(), Some(Two::Zero));
        assert_eq!(iter.next(), None);

        let mut array = LogicArray::<Four>::new(3, Four::HighImpedance);
        array.set(0, Four::One);

        let mut iter = array.iter();
        println!("{:#08b}", unsafe { *iter.ptr });
        assert_eq!(iter.next(), Some(Four::One));
        assert_eq!(iter.next(), Some(Four::HighImpedance));
        assert_eq!(iter.next(), Some(Four::HighImpedance));
        assert_eq!(iter.next(), None);

        let mut array = LogicArray::<Nine>::new(3, Nine::UnknownWeak);
        array.set(0, Nine::OneWeak);

        let mut iter = array.iter();
        assert_eq!(iter.next(), Some(Nine::OneWeak));
        assert_eq!(iter.next(), Some(Nine::UnknownWeak));
        assert_eq!(iter.next(), Some(Nine::UnknownWeak));
        assert_eq!(iter.next(), None);
    }
}
