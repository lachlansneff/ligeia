use std::{alloc::Allocator, marker::PhantomData, ops::Range, ptr::NonNull};

use crate::unit::{Header, Mut, Unit, UnitArray, UnitSlice};

/// Loosely based on https://github.com/trishume/gigatrace.
pub struct ImplicitForest<U: Unit, H: Header<U>, A: Allocator> {
    chunks: Vec<U::Chunk, A>,
    /// the number of units in each element.
    width: usize,
    chunks_per_element: usize,
    len: usize,
    _marker: PhantomData<H>,
}

impl<U: Unit, H: Header<U>, A: Allocator> ImplicitForest<U, H, A> {
    pub fn new_in(width: usize, alloc: A) -> Self {
        Self {
            chunks: Vec::new_in(alloc),
            width,
            chunks_per_element: H::CHUNKS + (width + U::PER_CHUNK - 1) / U::PER_CHUNK,
            len: 0,
            _marker: PhantomData,
        }
    }

    pub fn with_capacity_in(width: usize, capacity: usize, alloc: A) -> Self {
        Self {
            chunks: Vec::with_capacity_in(capacity, alloc),
            width,
            chunks_per_element: H::CHUNKS + (width + U::PER_CHUNK - 1) / U::PER_CHUNK,
            len: 0,
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, units: UnitSlice<U, H>) {
        debug_assert_eq!(self.len, self.chunks.len() / self.chunks_per_element);
        debug_assert_eq!(self.width, units.width());

        self.chunks.extend_from_slice(units.header().as_ref());
        self.chunks.extend_from_slice(units.chunks());
        self.len += 1;

        let len = self.len;
        let levels_to_index = len.trailing_ones() - 1;
        
        let mut current = len - 1;
        for level in 0..levels_to_index {
            let prev_higher_level = current - (1 << level);

            unsafe {
                let lhs: UnitSlice<U, H, Mut> = UnitSlice::new_inline_header(self.width, NonNull::from(&mut self.chunks[prev_higher_level]));
                let rhs: UnitSlice<U, H> = UnitSlice::new_inline_header(self.width, NonNull::from(&self.chunks[current]));

                H::aggregate(lhs, rhs);
            }

            current = prev_higher_level;
        }

        let new_agg_elem_index = len - (1 << levels_to_index);
        self.chunks.extend_from_within(new_agg_elem_index..new_agg_elem_index + self.chunks_per_element);
        self.len += 1;
    }

    pub fn range_query(&self, r: Range<usize>) -> UnitArray<U, H, A> {
        let mut combined = UnitArray::new(self.width, H::EMPTY, self.chunks.allocator());
        self.range_query_reuse(r, &mut combined);
        combined
    }

    fn range_query_reuse<'a>(&'a self, r: Range<usize>, combined: &mut UnitArray<'a, U, H, A>) {
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
            let agg_node_index = agg_node(range_interior.start, skip) * self.chunks_per_element;

            let agg_node: UnitSlice<U, H> = unsafe { UnitSlice::new_inline_header(self.width, NonNull::from(&self.chunks[agg_node_index])) };

            H::aggregate(combined.as_slice_mut(), agg_node);

            // let agg_node_ptr: *const U::Chunk = &self.chunks[agg_node_index * self.chunks_per_element];
            // unsafe {
            //     mix_element_ptrs::<U, M>(combined.as_mut_ptr(), agg_node_ptr, self.width, self.chunks_per_element);
            // }
            range_interior.start += skip;
        }
    }

    // /// mixes the element at `rhs_index` into the element at `lhs_index`.
    // fn mix_element(&mut self, lhs_index: usize, rhs_index: usize) {
    //     debug_assert_ne!(lhs_index, rhs_index, "attempted to mix elements that overlap with each other");

    //     let real_lhs_index = lhs_index * self.chunks_per_element;
    //     let real_rhs_index = rhs_index * self.chunks_per_element;

    //     let lhs_ptr: *mut U::Chunk = &mut self.chunks[real_lhs_index];
    //     let rhs_ptr: *const U::Chunk = &self.chunks[real_rhs_index];

    //     unsafe {
    //         mix_element_ptrs::<U, M>(lhs_ptr, rhs_ptr, self.width, self.chunks_per_element)
    //     }
    // }
}

// unsafe fn mix_element_ptrs<U: Unit, M: Mix<U>>(mut lhs: *mut U::Chunk, mut rhs: *const U::Chunk, units: usize, chunks_per_element: usize) {
//     let rem_units = units % U::PER_CHUNK;
//     let stage1_count = chunks_per_element - if rem_units == 0 { 0 } else { 1 };

//     for _ in 0..stage1_count {
//         unsafe {
//             *lhs = M::mix(*lhs, *rhs);
//             lhs = lhs.add(1);
//             rhs = rhs.add(1);
//         }
//     }

//     if rem_units != 0 {
//         unsafe {
//             *lhs = M::mix_partial(*lhs, *rhs, rem_units);
//         }
//     }
// }

#[cfg(test)]
mod tests {
    // use std::alloc::Global;
    // use crate::unit::{TwoLogic, Max};
    // use super::*;

    // #[test]
    // fn new_implicit_forest() {
    //     let _forest: ImplicitForest<TwoLogic, Max, Global> = ImplicitForest::new_in(1, Global);
    // }

    // #[test]
    // fn simple_range_query() {
    //     let mut forest: ImplicitForest<TwoLogic, Max, Global> = ImplicitForest::new_in(1, Global);

    //     forest.push(&[0b1]);
    //     forest.push(&[0b0]);
    //     println!("pushed");

    //     let a = forest.range_query(0..1);
    //     println!("queried");
    //     let v: Vec<TwoLogic> = a.iter().collect();
    //     println!("{:?}", v);
    // }
}