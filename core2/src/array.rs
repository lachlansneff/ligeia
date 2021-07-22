
use std::{alloc::Allocator, array, cmp, iter::Take, marker::PhantomData, mem::{self, MaybeUninit}, ptr::NonNull};
use crate::Unit;

#[derive(Clone, Copy)]
pub struct UnitArray<'a, U: Unit, A: Allocator> {
    chunks: NonNull<U::Chunk>,
    units: usize,
    alloc: &'a A,
}

impl<'a, U: Unit, A: Allocator> UnitArray<'a, U, A> {
    pub(crate) fn new(units: usize, alloc: &'a A) -> Self {
        let chunks_per_element = (units + U::PER_CHUNK - 1) / U::PER_CHUNK;

        let mut chunks_vec = Vec::with_capacity_in(chunks_per_element, alloc);
        chunks_vec.resize(chunks_per_element, U::EMPTY);
        let chunks = unsafe { NonNull::new_unchecked(chunks_vec.as_mut_ptr()) };
        mem::forget(chunks_vec);

        Self {
            chunks,
            units,
            alloc,
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut U::Chunk {
        self.chunks.as_ptr()
    }

    pub fn units(&self) -> usize {
        self.units
    }

    pub fn iter(&self) -> UnitIter<U> where [(); U::PER_CHUNK]: {
        UnitIter {
            chunks: self.chunks.as_ptr(),
            units: self.units,
            inner_iter: None,
            _marker: PhantomData,
        }
    }
}

pub struct UnitIter<'a, U: Unit> where [(); U::PER_CHUNK]: {
    chunks: *const U::Chunk,
    units: usize,
    inner_iter: Option<Take<array::IntoIter<U, { U::PER_CHUNK }>>>,
    _marker: PhantomData<&'a [U::Chunk]>,
}

impl<U: Unit> Iterator for UnitIter<'_, U> where [(); U::PER_CHUNK]: {
    type Item = U;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(iter) = self.inner_iter.as_mut() {
            if let Some(item) = iter.next() {
                self.units -= 1;
                return Some(item);
            }
        }

        if self.units > 0 {
            let c = unsafe {
                let c = *self.chunks;
                self.chunks = self.chunks.add(1);
                c
            };

            let count = cmp::min(self.units, U::PER_CHUNK);

            let mut iter = array::IntoIter::new(U::from_chunk(c)).take(count);
            let ret = iter.next().unwrap();
            self.units -= 1;
            self.inner_iter = Some(iter);
            Some(ret)
        } else {
            None
        }
    }
}