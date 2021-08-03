use std::{alloc::Allocator, marker::PhantomData, ops::Range};

use crate::{
    logic::{Combine, Logic, LogicArray},
    waves::{ChangeBlockList, ChangeOffset, Timesteps},
};

/// Loosely based on https://github.com/trishume/gigatrace.
pub struct ImplicitForest<L: Logic, C: Combine<L>, A: Allocator> {
    vals: Vec<ChangeOffset, A>,
    width: usize,
    _marker: PhantomData<(L, C)>,
}

impl<L: Logic, C: Combine<L>, A: Allocator> ImplicitForest<L, C, A> {
    pub fn new_in(width: usize, alloc: A) -> Self {
        Self {
            vals: Vec::new_in(alloc),
            width,
            _marker: PhantomData,
        }
    }

    pub fn push(
        &mut self,
        change_blocks: &mut ChangeBlockList<impl Allocator>,
        offset: ChangeOffset,
    ) {
        self.vals.push(offset);

        let len = self.vals.len();
        let levels_to_index = len.trailing_ones() - 1;

        let mut current = len - 1;
        for level in 0..levels_to_index {
            let prev_higher_level = current - (1 << level);

            let (lhs, rhs) = unsafe {
                change_blocks.get_two_changes(
                    self.vals[prev_higher_level],
                    self.vals[current],
                    self.width,
                )
            };

            C::combine(lhs, rhs);

            current = prev_higher_level;
        }

        self.vals.push(self.vals[len - (1 << levels_to_index)]);
    }

    pub fn range_query(
        &self,
        change_blocks: &ChangeBlockList<impl Allocator>,
        r: Range<usize>,
        combined: &mut LogicArray<L>,
    ) -> Timesteps {
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

        let mut ri = (r.start * 2)..(r.end * 2);
        let len = self.vals.len();
        assert!(
            ri.start <= len && ri.end <= len,
            "range {:?} not inside 0..{}",
            r,
            len / 2
        );

        assert_eq!(combined.width(), self.width);
        let mut combined_header = None;

        while ri.start < ri.end {
            let skip = largest_prefix_inside_skip(ri.start, ri.end);

            let (rhs_header, rhs_slice) = unsafe {
                change_blocks.get_change(self.vals[agg_node(ri.start, skip)], self.width)
            };

            C::combine(
                (
                    *combined_header.get_or_insert(rhs_header),
                    combined.as_slice_mut(),
                ),
                (rhs_header, rhs_slice),
            );
            ri.start += skip;
        }

        combined_header.unwrap()
    }
}
