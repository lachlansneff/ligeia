use std::{alloc::Allocator, marker::PhantomData};

use crate::{logic::{Combine, Logic}, unit::Unit, waves::{ChangeBlockList, ChangeHeader, ChangeOffset}};



pub enum Node {
    Empty,
    Indexed(ChangeOffset),
}

pub struct ImplicitForest<L: Logic, C: Combine<L>, A: Allocator> {
    vals: Vec<Node, A>,
    _marker: PhantomData<(L, C)>
}

impl<L: Logic, C: Combine<L>, A: Allocator> ImplicitForest<L, C, A> {
    pub fn new_in(alloc: A) -> Self {
        Self {
            vals: Vec::new_in(alloc),
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, change_blocks: &ChangeBlockList<impl Allocator>, offset: ChangeOffset) {
        self.vals.push(Node::Indexed(offset));

        let len = self.vals.len();
        let levels_to_index = len.trailing_ones() - 1;

        let current = len - 1;
        for level in 0..levels_to_index {
            let prev_higher_level = current - (1 << level);
            let combined = C::co
        }
    }
}
