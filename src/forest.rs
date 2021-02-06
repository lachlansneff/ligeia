// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{alloc::Allocator, collections::HashMap, sync::Arc};

use mmap_alloc::MmappableAllocator;

use crate::{db::VariableId, mmap_alloc, mmap_vec::{ReallocDisabled, VarMmapVec}, unsized_types::{Bit, BitType, KnownUnsized, KnownUnsizedVec, Qit, ValueChangeNode}};

type Alloc = MmappableAllocator;

// pub enum Tree {
//     BitTree(KnownUnsizedVec<ValueChangeNode<Bit>, Alloc>),
//     QitTree(KnownUnsizedVec<ValueChangeNode<Qit>, Alloc>),
//     Utf8Tree()
// }

pub struct Tree<T: BitType> {
    nodes: KnownUnsizedVec<ValueChangeNode<T>, Alloc>,
}

pub enum TypedTree {
    BitTree(Tree<Bit>),
    QitTree(Tree<Qit>),
}

impl<T: BitType> Tree<T> {

}

/// Contains a forest of trees of value change layers.
pub struct Forest {
    allocator: Alloc,
    trees: HashMap<VariableId, TypedTree>,
}


