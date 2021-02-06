// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{alloc::{Allocator, Layout}, fmt::{Display, Debug}, marker::PhantomData, mem, ops::{Index, IndexMut, RangeBounds}, ptr::{self, NonNull}, slice};
// use rfc2580::{MetaData, Pointee, from_raw_parts, into_raw_parts};

pub trait BitType: Copy + Clone + PartialEq + Eq + Display {
    const BIT_SIZE: u8;
    const MASK: u8;
    const FORMAT_PREFIX: &'static str;
    const UNINIT_BYTE: u8;

    const PER_BYTE: u8 = 8 / Self::BIT_SIZE;

    fn from_bits(bits: u8) -> Self;
    fn to_bits(self) -> u8;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bit {
    Zero = 0,
    One = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Qit {
    Zero = 0,
    One = 1,
    X = 2,
    Z = 3,
}

impl Display for Bit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bit::Zero => write!(f, "0"),
            Bit::One => write!(f, "1"),
        }
    }
}
impl BitType for Bit {
    const BIT_SIZE: u8 = 1;
    const MASK: u8 = 0b1;
    const FORMAT_PREFIX: &'static str = "b";
    const UNINIT_BYTE: u8 = 0b0000_0000;

    fn from_bits(bits: u8) -> Self {
        match bits & Self::MASK {
            0b0 => Bit::Zero,
            0b1 => Bit::One,
            _ => unreachable!()     
        }
    }

    fn to_bits(self) -> u8 {
        self as u8
    }
}
impl Display for Qit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Qit::Zero => write!(f, "0"),
            Qit::One => write!(f, "1"),
            Qit::X => write!(f, "x"),
            Qit::Z => write!(f, "z"),
        }
    }
}
impl BitType for Qit {
    const BIT_SIZE: u8 = 2;
    const MASK: u8 = 0b11;
    const FORMAT_PREFIX: &'static str = "q";
    const UNINIT_BYTE: u8 = 0b1010_1010;

    fn from_bits(bits: u8) -> Self {
        match bits & Self::MASK {
            0 => Qit::Zero,
            1 => Qit::One,
            2 => Qit::X,
            3 => Qit::Z,
            _ => unreachable!()     
        }
    }

    fn to_bits(self) -> u8 {
        self as u8
    }
}

pub unsafe trait KnownUnsized {
    type Parts;
    type Bit;

    fn size_from_meta(meta: usize) -> usize;
    fn align() -> usize;
    fn from_raw_parts(ptr: *const u8, meta: usize) -> *const Self;
    fn from_raw_parts_mut(ptr: *mut u8, meta: usize) -> *mut Self;

    unsafe fn write_from_parts(ptr: *mut Self, bit_slice: &BitSlice<Self::Bit>, parts: Self::Parts);
    unsafe fn write_from_iter(ptr: *mut Self, iter: impl Iterator<Item = Self::Bit>, parts: Self::Parts);
}

// struct BitSliceMetaData<T: BitType>(usize, PhantomData<fn(T) -> T>);

// impl<T: BitType> MetaData for BitSliceMetaData<T> {
//     type Pointee = BitSlice<T>;

//     fn assemble(&self, data: *const u8) -> *const BitSlice<T> {
//         assert!(mem::size_of::<*const BitSlice<T>>() == mem::size_of::<(usize, usize)>());

//         unsafe { mem::transmute((data, self.0)) }
//     }

//     fn disassemble(ptr: *const BitSlice<T>) -> (Self, *const u8) {
//         assert!(mem::size_of::<*const BitSlice<T>>() == mem::size_of::<(usize, usize)>());
        
//         let (data, len): (usize, usize) = unsafe { mem::transmute(ptr) };

//         (BitSliceMetaData(len, PhantomData), data as *const u8)
//     }
// }

/// Someday, this will be a real custom DST. Right now, it's an abomination.
/// Using `mem::size_of_val` or similar will not return correct values for this.
pub struct BitSlice<T> {
    _marker: PhantomData<[T]>,
    // Is this undefined behavior? The len is actually longer than the byte len.
    data: [u8],
}

impl<T: BitType> BitSlice<T> {
    pub fn new<'a>(len: usize, bytes: &'a [u8]) -> &'a Self {
        assert!(bytes.len() * T::PER_BYTE as usize >= len);

        // SAFETY: Miri really does not like this, but as long as `data` is not
        // used without `bytes()` or `bytes_mut()`, this should be okay.
        unsafe { &*(ptr::slice_from_raw_parts(bytes.as_ptr(), len) as *const BitSlice<T>) }
    }

    pub fn new_mut<'a>(len: usize, bytes: &'a mut [u8]) -> &'a mut Self {
        assert!(bytes.len() * T::PER_BYTE as usize >= len);

        // SAFETY: Miri really does not like this, but as long as `data` is not
        // used without `bytes()` or `bytes_mut()`, this should be okay.
        unsafe { &mut *(ptr::slice_from_raw_parts_mut(bytes.as_mut_ptr(), len) as *mut BitSlice<T>) }
    }

    pub fn new_boxed(len: usize) -> Box<Self> {
        let bytes = (len + T::PER_BYTE as usize - 1) / T::PER_BYTE as usize;
        assert!(bytes * T::PER_BYTE as usize >= len);
        let slice = Box::leak(vec![T::UNINIT_BYTE; bytes].into_boxed_slice());
        unsafe { Box::from_raw(Self::new_mut(len, slice)) }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn bytes(&self) -> &[u8] {
        let bytes = (self.len() + T::PER_BYTE as usize - 1) / T::PER_BYTE as usize;
        
        unsafe { slice::from_raw_parts(self.data.as_ptr(), bytes) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        let bytes = (self.len() + T::PER_BYTE as usize - 1) / T::PER_BYTE as usize;
        
        unsafe { slice::from_raw_parts_mut(self.data.as_mut_ptr(), bytes) }
    }

    /// Update this in-place.
    pub fn update<F: FnMut(T) -> T>(&mut self, mut f: F) {
        for byte in self.bytes_mut() {
            for i in 0..T::PER_BYTE as usize {
                let in_index = i * T::BIT_SIZE as usize;
                let bit = (*byte & (T::MASK << in_index)) >> in_index;
                let this_bit = T::from_bits(bit);

                *byte = (*byte & !(T::MASK << in_index)) | (T::to_bits(f(this_bit)) << in_index);
            }
        }
    }

    /// Mix this in-place with another bit slice.
    pub fn mix<F: FnMut(T, T) -> T>(&mut self, other: &Self, mut f: F) {
        assert_eq!(self.len(), other.len());
        let mut iter = other.into_iter();

        for byte in self.bytes_mut() {
            for (i, other_bit) in (&mut iter).take(T::PER_BYTE as usize).enumerate() {
                let in_index = i * T::BIT_SIZE as usize;
                let bit = (*byte & (T::MASK << in_index)) >> in_index;
                let this_bit = T::from_bits(bit);

                *byte = (*byte & !(T::MASK << in_index)) | (T::to_bits(f(this_bit, other_bit)) << in_index);
            }
        }
    }
}

unsafe impl<T: BitType> KnownUnsized for BitSlice<T> {
    type Parts = ();
    type Bit = T;

    fn size_from_meta(meta: usize) -> usize {
        (meta + T::PER_BYTE as usize - 1) / T::PER_BYTE as usize
    }

    fn align() -> usize {
        1
    }

    fn from_raw_parts(ptr: *const u8, meta: usize) -> *const Self {
        ptr::slice_from_raw_parts(ptr, meta) as *const Self
    }

    fn from_raw_parts_mut(ptr: *mut u8, meta: usize) -> *mut Self {
        ptr::slice_from_raw_parts_mut(ptr, meta) as *mut Self
    }

    unsafe fn write_from_parts(ptr: *mut Self, slice: &BitSlice<T>, _: ()) {
        (&mut *ptr).bytes_mut().copy_from_slice(slice.bytes());
    }

    unsafe fn write_from_iter(ptr: *mut Self, mut iter: impl Iterator<Item = Self::Bit>, _: ()) {
        for byte in (&mut *ptr).bytes_mut() {
            for (i, other_bit) in (&mut iter).take(T::PER_BYTE as usize).enumerate() {
                let in_index = i * T::BIT_SIZE as usize;

                *byte = (*byte & !(T::MASK << in_index)) | (T::to_bits(other_bit) << in_index);
            }
        }
    }
}

impl<'a, T: BitType> IntoIterator for &'a BitSlice<T> {
    type Item = T;

    type IntoIter = BitSliceIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        BitSliceIter {
            size: self.len(),
            index: 0,
            bytes: self.bytes(),
            _marker: PhantomData,   
        }
    }
}

pub struct BitSliceIter<'a, T> {
    size: usize,
    index: usize,
    bytes: &'a [u8],
    _marker: PhantomData<T>,
}

impl<T: BitType> Iterator for BitSliceIter<'_, T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.size {
            let byte = self.bytes[(self.index as usize) / 8];
            let in_index = (self.index * T::BIT_SIZE as usize) % 8;
            let bit = (byte & (T::MASK << in_index)) >> in_index;
            self.index += 1;

            Some(T::from_bits(bit))
        } else {
            None
        }
    }
}

impl<T: BitType> Display for BitSlice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for bit in self {
            bit.fmt(f)?;
        }
        Ok(())
    }
}

impl<T: BitType> Debug for BitSlice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0{}{}", T::FORMAT_PREFIX, self)
    }
}
/// Using `mem::size_of_val` or similar will not return correct values for this.
pub struct ValueChangeNode<T> {
    pub timestamp: u64,
    pub bits: BitSlice<T>,
}

impl<T: BitType> ValueChangeNode<T> {
    // /// Create a value change node unsafely. Be especially careful with this,
    // /// as the returned lifetime can be infinitely long.
    // pub unsafe fn from_raw<'a>(len: usize, raw: *const Self) -> &'a Self {
    //     unsafe { &*(slice::from_raw_parts(raw as *const u8, len) as *const [u8] as *const Self) }
    // }
}

unsafe impl<T: BitType> KnownUnsized for ValueChangeNode<T> {
    type Parts = u64;
    type Bit = T;

    fn size_from_meta(meta: usize) -> usize {
        mem::size_of::<u64>() + <BitSlice::<T> as KnownUnsized>::size_from_meta(meta)
    }

    fn align() -> usize {
        mem::align_of::<u64>()
    }

    fn from_raw_parts(ptr: *const u8, meta: usize) -> *const Self {
        ptr::slice_from_raw_parts(ptr, meta) as *const Self
    }

    fn from_raw_parts_mut(ptr: *mut u8, meta: usize) -> *mut Self {
        ptr::slice_from_raw_parts_mut(ptr, meta) as *mut Self
    }

    unsafe fn write_from_parts(ptr: *mut Self, bit_slice: &BitSlice<T>, timestamp: u64) {
        let dest = &mut *ptr;
        dest.timestamp = timestamp;
        dest.bits.bytes_mut().copy_from_slice(bit_slice.bytes());
    }

    unsafe fn write_from_iter(ptr: *mut Self, mut iter: impl Iterator<Item = Self::Bit>, timestamp: u64) {
        let mut dest = &mut *ptr;
        dest.timestamp = timestamp;
        for byte in dest.bits.bytes_mut() {
            for (i, other_bit) in (&mut iter).take(T::PER_BYTE as usize).enumerate() {
                let in_index = i * T::BIT_SIZE as usize;

                *byte = (*byte & !(T::MASK << in_index)) | (T::to_bits(other_bit) << in_index);
            }
        }
    }
}

pub struct KnownUnsizedVec<T: ?Sized + KnownUnsized, A: Allocator> {
    element_size: usize,
    meta: usize,
    len: usize,
    cap: usize,
    ptr: NonNull<u8>,
    alloc: A,
    _marker: PhantomData<NonNull<T>>,
}

impl<T: ?Sized + KnownUnsized, A: Allocator> KnownUnsizedVec<T, A> {
    pub fn with_capacity_in(meta: usize, cap: usize, alloc: A) -> Self {
        let (layout, element_size) = Layout::from_size_align(T::size_from_meta(meta), T::align())
            .unwrap()
            .repeat(cap)
            .unwrap();

        let ptr = alloc.allocate(layout).unwrap();

        Self {
            element_size,
            meta,
            len: 0,
            cap,
            ptr: ptr.as_non_null_ptr(),
            alloc,
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, slice: &BitSlice<T::Bit>, parts: T::Parts) {
        if self.len == self.cap {
            self.realloc();
        }

        let ptr = unsafe { self.ptr.as_ptr().add(self.len * self.element_size) };
        let dest = T::from_raw_parts_mut(ptr, self.meta);
        unsafe { T::write_from_parts(dest, slice, parts) }
        self.len += 1;
    }

    pub fn push_from_iter(&mut self, iter: impl Iterator<Item = T::Bit>, parts: T::Parts) {
        if self.len == self.cap {
            self.realloc();
        }

        let ptr = unsafe { self.ptr.as_ptr().add(self.len * self.element_size) };
        let dest = T::from_raw_parts_mut(ptr, self.meta);
        unsafe { T::write_from_iter(dest, iter, parts) }
        self.len += 1;
    }

    #[cold]
    fn realloc(&mut self) {
        let old_cap = self.cap;
        if self.cap == 0 {
            self.cap = 1;
        } else {
            self.cap *= 2;
        }

        let layout = Layout::from_size_align(T::size_from_meta(self.meta), T::align()).unwrap();

        let (old_layout, _) = layout.repeat(old_cap).unwrap();

        let (new_layout, _) = layout.repeat(self.cap).unwrap();

        let ptr = unsafe {
            self.alloc.grow(self.ptr, old_layout, new_layout).expect("failed to grow KnownUnsizedVec")
        };

        self.ptr = ptr.as_non_null_ptr();
    }

    pub fn iter<B: RangeBounds<usize>>(&self, bounds: B) -> KnownUnsizedVecIter<T> {
        let bounds = bounds.assert_len(self.len);
        KnownUnsizedVecIter {
            element_size: self.element_size,
            meta: self.meta,
            ptr: unsafe { NonNull::new_unchecked(self.ptr.as_ptr().add(bounds.start * self.element_size)) },
            end: unsafe { self.ptr.as_ptr().add(bounds.end * self.element_size) },
            _marker: PhantomData,
        }
    }

    pub fn iter_mut<B: RangeBounds<usize>>(&mut self, bounds: B) -> KnownUnsizedVecIterMut<T> {
        let bounds = bounds.assert_len(self.len);
        KnownUnsizedVecIterMut {
            element_size: self.element_size,
            meta: self.meta,
            ptr: unsafe { NonNull::new_unchecked(self.ptr.as_ptr().add(bounds.start * self.element_size)) },
            end: unsafe { self.ptr.as_ptr().add(bounds.end * self.element_size) },
            _marker: PhantomData,
        }
    }
}

// impl<T: ?Sized + KnownUnsized, A: Allocator> IntoIterator for KnownUnsizedVec<T, A>

impl<T: ?Sized + KnownUnsized, A: Allocator> Drop for KnownUnsizedVec<T, A> {
    fn drop(&mut self) {
        let (layout, _) = Layout::from_size_align(self.element_size, T::align())
            .unwrap()
            .repeat(self.cap)
            .unwrap();
        
        unsafe { self.alloc.deallocate(self.ptr, layout) }
    }
}

impl<T: ?Sized + KnownUnsized, A: Allocator> Index<usize> for KnownUnsizedVec<T, A> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        unsafe {
            let ptr = self.ptr.as_ptr().add(index * self.element_size);

            &*T::from_raw_parts(ptr, self.meta)
        }
    }
}

impl<T: ?Sized + KnownUnsized, A: Allocator> IndexMut<usize> for KnownUnsizedVec<T, A> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        unsafe {
            let ptr = self.ptr.as_ptr().add(index * self.element_size);

            &mut *T::from_raw_parts_mut(ptr, self.meta)
        }
    }
}

impl<T: ?Sized + KnownUnsized + Debug, A: Allocator> Debug for KnownUnsizedVec<T, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter(..)).finish()
    }
}

// impl<T: ?Sized + KnownUnsized, A: Allocator> Index<Range<usize>> for KnownUnsizedVec<T, A> {
//     type Output = T;

//     fn index(&self, index: Range<usize>) -> &Self::Output {
//         unsafe {
//             let start_ptr = self.ptr.as_ptr().add(index.start * self.element_size);
//             let end_ptr = self.ptr.as_ptr().add(index.end * self.element_size);

//             &*T::from_raw_parts(ptr, self.meta)
//         }
//     }
// }

pub struct KnownUnsizedVecIter<'a, T: ?Sized> {
    element_size: usize,
    meta: usize,
    ptr: NonNull<u8>,
    end: *const u8,
    _marker: PhantomData<&'a T>,
}

impl<'a, T: ?Sized + KnownUnsized + 'a> Iterator for KnownUnsizedVecIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        if self.ptr.as_ptr() as *const _ == self.end {
            return None;
        }

        let item = unsafe { &*T::from_raw_parts(self.ptr.as_ptr(), self.meta) };
        self.ptr = unsafe { NonNull::new_unchecked(self.ptr.as_ptr().add(self.element_size)) };

        Some(item)
    }
}

impl<'a, T: ?Sized + KnownUnsized + 'a> DoubleEndedIterator for KnownUnsizedVecIter<'a, T> {
    fn next_back(&mut self) -> Option<&'a T> {
        if self.ptr.as_ptr() as *const _ == self.end {
            return None;
        }

        self.end = unsafe { self.end.sub(self.element_size) };
        let item = unsafe { &*T::from_raw_parts(self.end, self.meta) };

        Some(item)
    }
}

pub struct KnownUnsizedVecIterMut<'a, T: ?Sized> {
    element_size: usize,
    meta: usize,
    ptr: NonNull<u8>,
    end: *mut u8,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T: ?Sized + KnownUnsized + 'a> Iterator for KnownUnsizedVecIterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<&'a mut T> {
        if self.ptr.as_ptr() as *const _ == self.end {
            return None;
        }

        let item = unsafe { &mut *T::from_raw_parts_mut(self.ptr.as_ptr(), self.meta) };
        self.ptr = unsafe { NonNull::new_unchecked(self.ptr.as_ptr().add(self.element_size)) };

        Some(item)
    }
}

impl<'a, T: ?Sized + KnownUnsized + 'a> DoubleEndedIterator for KnownUnsizedVecIterMut<'a, T> {
    fn next_back(&mut self) -> Option<&'a mut T> {
        if self.ptr.as_ptr() as *const _ == self.end {
            return None;
        }

        self.end = unsafe { self.end.sub(self.element_size) };
        let item = unsafe { &mut *T::from_raw_parts_mut(self.end, self.meta) };

        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_slice_uninit() {
        let s = BitSlice::<Qit>::new_boxed(15);
        assert!(s.into_iter().all(|q| q == Qit::X), "uninit qit failed");

        let s = BitSlice::<Bit>::new_boxed(15);
        assert!(s.into_iter().all(|b| b == Bit::Zero), "uninit bit failed");
    }

    #[test]
    fn bit_slice_update() {
        let mut s = BitSlice::<Bit>::new_boxed(42);
        s.update(|_| Bit::One);
        assert!(s.into_iter().all(|b| b == Bit::One), "bit update failed");

        let mut s = BitSlice::<Qit>::new_boxed(15);
        s.update(|_| Qit::One);
        assert!(s.into_iter().all(|b| b == Qit::One), "bit update failed");
    }

    #[test]
    fn bit_slice_mix() {
        let mut s = BitSlice::<Bit>::new_boxed(42);
        let mut s2 = BitSlice::<Bit>::new_boxed(42);
        s2.update(|_| Bit::One);

        s.mix(&s2, |_b, b2| b2);

        assert!(s.into_iter().all(|b| b == Bit::One), "bit update failed");
    }

    #[test]
    fn value_change_aligned_slice() {
        let dummy_vc = ValueChangeNode::<Bit>::from_raw_parts(NonNull::dangling().as_ptr(), 0);
        let &ValueChangeNode::<Bit> { ref timestamp, ref bits } = unsafe { &*dummy_vc };
        
        assert_eq!(unsafe { (bits as *const _ as *const u8).offset_from(timestamp as *const _ as *const u8) } as usize, mem::size_of::<u64>());
    }

    #[test]
    fn known_unsized_vec_push() {
        let mut s = BitSlice::<Qit>::new_boxed(15);
        s.update(|_| Qit::One);
        let s2 = BitSlice::<Qit>::new_boxed(15);

        let mmappable = crate::mmap_alloc::MmappableAllocator::new();
        
        let mut v = KnownUnsizedVec::<BitSlice<Qit>, _>::with_capacity_in(15, 2, &mmappable);
        v.push(&s, ());
        v.push(&s2, ());

        assert_eq!(v[0].into_iter().next(), Some(Qit::One));

        assert_eq!(v.iter(1..).next_back().and_then(|s| s.into_iter().next()), Some(Qit::X));
    }
}
