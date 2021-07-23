use std::{alloc::Allocator, marker::PhantomData, mem, ptr::NonNull};

use crate::unit::{Unit, Header, UnitSlice, Mut};

pub struct UnitArray<'a, U: Unit, H: Header<U>, A: Allocator> {
    header: H,
    chunks: NonNull<U::Chunk>,
    width: usize,
    alloc: &'a A,
}

impl<'a, U: Unit, H: Header<U>, A: Allocator> UnitArray<'a, U, H, A> {
    pub(crate) fn new(width: usize, header: H, alloc: &'a A) -> Self {
        let chunks_per_element = (width + U::PER_CHUNK - 1) / U::PER_CHUNK;

        let mut chunks_vec = Vec::with_capacity_in(chunks_per_element, alloc);
        chunks_vec.resize(chunks_per_element, U::EMPTY);
        let chunks = unsafe { NonNull::new_unchecked(chunks_vec.as_mut_ptr()) };
        mem::forget(chunks_vec);

        Self {
            header,
            chunks,
            width,
            alloc,
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut U::Chunk {
        self.chunks.as_ptr()
    }

    pub fn header(&self) -> H {
        self.header
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn as_slice(&self) -> UnitSlice<U, H> {
        UnitSlice {
            header: self.header,
            chunks: self.chunks,
            width: self.width,
            _marker: PhantomData,
        }
    }

    pub fn as_slice_mut(&mut self) -> UnitSlice<U, H, Mut> {
        UnitSlice {
            header: self.header,
            chunks: self.chunks,
            width: self.width,
            _marker: PhantomData,
        }
    }
}

impl<'a, U: Unit, H: Header<U>, A: Allocator> Drop for UnitArray<'a, U, H, A> {
    fn drop(&mut self) {
        let chunks_per_element = (self.width + U::PER_CHUNK - 1) / U::PER_CHUNK;
        drop(unsafe {
            Vec::from_raw_parts_in(self.chunks.as_ptr(), chunks_per_element, chunks_per_element, self.alloc)
        });
    }
}