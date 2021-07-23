use std::{marker::PhantomData, ptr::NonNull, slice};
use crate::unit::{Header, Unit};

pub struct Mut;
pub struct Immut;
pub unsafe trait Mutability<'a, T> {
    type Ref;
}
unsafe impl<'a, T: 'a> Mutability<'a, T> for Mut {
    type Ref = &'a mut [T];
}
unsafe impl<'a, T: 'a> Mutability<'a, T> for Immut {
    type Ref = &'a [T];
}

pub struct UnitSlice<'a, U: Unit, H: Header<U>, M: Mutability<'a, U::Chunk> = Immut> where U::Chunk: 'a {
    pub(super) header: H,
    pub(super) chunks: NonNull<U::Chunk>,
    pub(super) width: usize,
    pub(super) _marker: PhantomData<M::Ref>,
}

impl<'a, U: Unit, H: Header<U>, M: Mutability<'a, U::Chunk>> UnitSlice<'a, U, H, M> {
    pub(crate) unsafe fn new_inline_header(width: usize, chunks: NonNull<U::Chunk>) -> Self {
        let slice_len = (width + U::PER_CHUNK - 1) / U::PER_CHUNK;
        let slice = unsafe { slice::from_raw_parts(chunks.as_ptr(), slice_len) };
        let (header_chunks, rest) = slice.split_at(H::CHUNKS);
        
        Self {
            header: H::from_chunks(header_chunks),
            chunks: unsafe { NonNull::new_unchecked(rest.as_ptr() as *mut _) },
            width,
            _marker: PhantomData,
        }
    }

    pub fn header(&self) -> H {
        self.header
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn chunks(&self) -> &[U::Chunk] {
        let len = (self.width + U::PER_CHUNK - 1) / U::PER_CHUNK;
        unsafe {
            slice::from_raw_parts(self.chunks.as_ptr(), len)
        }
    }

    pub fn iter(&self) -> UnitIter<U> {
        UnitIter {
            chunks: self.chunks.as_ptr(),
            width: self.width,
            offset: 0,
            _marker: PhantomData,
        }
    }

    pub fn get(&self, offset: usize) -> U {
        assert!(offset < self.width, "UnitSlice::get: offset out of bounds of width");
        unsafe {
            U::get_unit(self.chunks.as_ptr(), offset)
        }
    }
}

impl<'a, U: Unit, H: Header<U>> UnitSlice<'a, U, H, Mut> {
    pub fn set(&mut self, offset: usize, unit: U) {
        assert!(offset < self.width, "UnitSlice::set: offset out of bounds of width");
        unsafe {
            U::set_unit(self.chunks.as_ptr(), offset, unit)
        }
    }

    pub fn into_immut(self) -> UnitSlice<'a, U, H, Immut> {
        UnitSlice {
            header: self.header,
            chunks: self.chunks,
            width: self.width,
            _marker: PhantomData,
        }
    }
}

impl<'a, U: Unit, H: Header<U>> Clone for UnitSlice<'a, U, H, Immut> {
    fn clone(&self) -> Self {
        Self {
            ..*self
        }
    }
}

impl<'a, U: Unit, H: Header<U>> Copy for UnitSlice<'a, U, H, Immut> {}

pub struct UnitIter<'a, U: Unit> {
    chunks: *const U::Chunk,
    width: usize,
    offset: usize,
    _marker: PhantomData<&'a [U::Chunk]>,
}

impl<U: Unit> Iterator for UnitIter<'_, U> {
    type Item = U;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset < self.width {
            let unit = unsafe { U::get_unit(self.chunks, self.offset) };
            self.offset += 1;
            Some(unit)
        } else {
            None
        }
    }
}
