use std::{alloc::Allocator, marker::PhantomData, mem, ops::Range};

use crate::{Merge, Mix, Unit, array::UnitArray};

/// Loosely based on https://github.com/trishume/gigatrace.
pub struct ImplicitForest<U: Unit, M: Merge<U>, A: Allocator> {
    chunks: Vec<U::Chunk, A>,
    /// the number of units in each element.
    units: usize,
    chunks_per_element: usize,
    len: usize,
    _marker: PhantomData<M>,
}

impl<U: Unit, M: Merge<U>, A: Allocator> ImplicitForest<U, M, A> {
    const HEADER_CHUNKS: usize = mem::size_of::<M::Header>() / mem::size_of::<U::Chunk>();

    pub fn new_in(units: usize, alloc: A) -> Self {
        Self {
            chunks: Vec::new_in(alloc),
            units,
            chunks_per_element: Self::HEADER_CHUNKS + (units + U::PER_CHUNK - 1) / U::PER_CHUNK,
            len: 0,
            _marker: PhantomData,
        }
    }

    pub fn with_capacity_in(units: usize, capacity: usize, alloc: A) -> Self {
        Self {
            chunks: Vec::with_capacity_in(capacity, alloc),
            units,
            chunks_per_element: Self::HEADER_CHUNKS + (units + U::PER_CHUNK - 1) / U::PER_CHUNK,
            len: 0,
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, header: M::Header, chunks: &[U::Chunk]) {
        debug_assert_eq!(self.len, self.chunks.len() / self.chunks_per_element);
        debug_assert_eq!(chunks.len(), self.chunks_per_element, "mismatched chunks length and the number of chunks for element");

        self.chunks.extend_from_slice(header.as_ref());
        self.chunks.extend_from_slice(chunks);
        if let Some(last) = self.chunks.last_mut() {
            let rem = self.units % U::PER_CHUNK;
            if rem != 0 {
                *last = U::mask_off_extra(*last, rem);
            }
        }
        self.len += 1;

        let len = self.len;
        let levels_to_index = len.trailing_ones() - 1;
        
        let mut current = len - 1;
        for level in 0..levels_to_index {
            let prev_higher_level = current - (1 << level);
            self.mix_element(prev_higher_level, current);
            current = prev_higher_level;
        }

        let new_agg_elem_index = len - (1 << levels_to_index);
        self.chunks.extend_from_within(new_agg_elem_index..new_agg_elem_index + self.chunks_per_element);
        self.len += 1;
    }

    pub fn range_query(&self, r: Range<usize>) -> UnitArray<U, A> {
        let mut combined = UnitArray::new(self.units, self.chunks.allocator());
        self.range_query_reuse(r, &mut combined);
        combined
    }

    fn range_query_reuse<'a>(&'a self, r: Range<usize>, combined: &mut UnitArray<'a, U, A>) {
        fn lsp(x: usize) -> usize {
            x & x.wrapping_neg()
        }
        fn msp(x: usize) -> usize {
            1usize.reverse_bits() >> x.leading_zeros()
        }
        fn largest_prefix_inside_skip(min: usize, max: usize) -> usize {
            lsp(min | msp(max - min))
        }
        fn agg_node(i: usize, offset: usize) -> usize {
            i + (offset >> 1) - 1
        }

        let mut range_interior = (r.start * 2)..(r.end * 2);
        let len = self.len;
        assert!(range_interior.start <= len && range_interior.end <= len, "range {:?} not inside 0..{}", r, len / 2);

        while range_interior.start < range_interior.end {
            let skip = largest_prefix_inside_skip(range_interior.start, range_interior.end);
            let agg_node_index = agg_node(range_interior.start, skip);
            let agg_node_ptr: *const U::Chunk = &self.chunks[agg_node_index * self.chunks_per_element];
            unsafe {
                mix_element_ptrs::<U, M>(combined.as_mut_ptr(), agg_node_ptr, self.units, self.chunks_per_element);
            }
            range_interior.start += skip;
        }
    }

    /// mixes the element at `rhs_index` into the element at `lhs_index`.
    fn mix_element(&mut self, lhs_index: usize, rhs_index: usize) {
        debug_assert_ne!(lhs_index, rhs_index, "attempted to mix elements that overlap with each other");

        let real_lhs_index = lhs_index * self.chunks_per_element;
        let real_rhs_index = rhs_index * self.chunks_per_element;

        let lhs_ptr: *mut U::Chunk = &mut self.chunks[real_lhs_index];
        let rhs_ptr: *const U::Chunk = &self.chunks[real_rhs_index];

        unsafe {
            mix_element_ptrs::<U, M>(lhs_ptr, rhs_ptr, self.units, self.chunks_per_element)
        }
    }
}

unsafe fn mix_element_ptrs<U: Unit, M: Mix<U>>(mut lhs: *mut U::Chunk, mut rhs: *const U::Chunk, units: usize, chunks_per_element: usize) {
    let rem_units = units % U::PER_CHUNK;
    let stage1_count = chunks_per_element - if rem_units == 0 { 0 } else { 1 };

    for _ in 0..stage1_count {
        unsafe {
            *lhs = M::mix(*lhs, *rhs);
            lhs = lhs.add(1);
            rhs = rhs.add(1);
        }
    }

    if rem_units != 0 {
        unsafe {
            *lhs = M::mix_partial(*lhs, *rhs, rem_units);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::alloc::Global;
    use crate::{Bit, Max};
    use super::*;

    #[test]
    fn new_implicit_forest() {
        let _forest: ImplicitForest<Bit, Max, Global> = ImplicitForest::new_in(1, Global);
    }

    #[test]
    fn simple_range_query() {
        let mut forest: ImplicitForest<Bit, Max, Global> = ImplicitForest::new_in(1, Global);

        forest.push(&[0b1]);
        forest.push(&[0b0]);
        println!("pushed");

        let a = forest.range_query(0..1);
        println!("queried");
        let v: Vec<Bit> = a.iter().collect();
        println!("{:?}", v);
    }
}