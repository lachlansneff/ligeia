// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{
    info_bars::InfoBars,
    lazy::LazyModify,
    mmap_alloc::MmappableAllocator,
    unsized_types::{Bit, BitSlice, BitType, KnownUnsizedVec, Qit, ValueChangeNode},
};
use std::{
    collections::HashMap,
    future::Future,
    io::Read,
    iter,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
pub struct Scope {
    pub name: String,
    pub variables: Vec<Variable>,
    pub scopes: Vec<Scope>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum VariableInfo {
    Integer {
        bits: usize,
        is_signed: bool,
    },
    Enum {
        bits: usize,
        fields: Vec<(String, Box<BitSlice<Bit>>)>,
    },
    String {
        len: usize,
    },
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct VariableId(pub u64);

#[derive(Debug)]
pub struct Variable {
    pub id: VariableId,
    pub name: String,
    pub info: VariableInfo,
}

pub trait WaveformLoader: Sync {
    fn supports_file_extension(&self, s: &str) -> bool;
    fn description(&self) -> String;

    /// A file is technically a stream, but generally, specializing parsers for files can be more efficient than parsing
    /// from a generic reader.
    fn load_file(&self, info_bars: &InfoBars, path: &Path) -> anyhow::Result<Waveform>;
    fn load_stream(&self, info_bars: &InfoBars, reader: &mut dyn Read) -> anyhow::Result<Waveform>;
}

type Alloc = MmappableAllocator;

// pub enum Tree {
//     BitTree(KnownUnsizedVec<ValueChangeNode<Bit>, Alloc>),
//     QitTree(KnownUnsizedVec<ValueChangeNode<Qit>, Alloc>),
//     Utf8Tree()
// }

pub struct Tree<T: BitType> {
    node_tree: KnownUnsizedVec<ValueChangeNode<T>, Alloc>,
}

pub enum TreeOrLayer<T: BitType> {
    Tree(Tree<T>),
    Layer(KnownUnsizedVec<ValueChangeNode<T>, Alloc>),
}

impl<T: BitType> TreeOrLayer<T> {
    fn into_tree(self) -> Self {
        match self {
            TreeOrLayer::Layer(layer) => TreeOrLayer::Tree(Tree::new(layer)),
            x @ TreeOrLayer::Tree(_) => x,
        }
    }
}

fn layer_count_generator(first_layer_len: usize) -> impl Iterator<Item = usize> {
    let mut layer_len = first_layer_len;

    iter::from_fn(move || {
        if layer_len < 1024 {
            return None;
        }

        layer_len /= 4;

        Some(layer_len)
    })
}

impl<T: BitType> Tree<T> {
    pub fn new(first_layer: KnownUnsizedVec<ValueChangeNode<T>, Alloc>) -> Self {
        let mut node_tree = first_layer;
        let real_data_len = node_tree.len();
        let additional_len_required = layer_count_generator(real_data_len).sum();
        node_tree.reserve(additional_len_required);

        let mut averaged_bits = BitSlice::<T>::new_boxed(node_tree.meta());

        let previous_layer_len = real_data_len;
        for layer_len in layer_count_generator(real_data_len) {
            let mut previous_layer_index_iter = 0..previous_layer_len;

            for _ in 0..layer_len {
                let mut summed_timestamp = 0u128;

                for (i, index) in (&mut previous_layer_index_iter).take(4).enumerate() {
                    let node = &node_tree[index];

                    if i == 0 {
                        averaged_bits.copy_from(&node.bits);
                        summed_timestamp = node.timestamp as u128;
                    } else {
                        summed_timestamp += node.timestamp as u128;
                        averaged_bits.mix(&node.bits, T::average);
                    }
                }

                node_tree.push(&*averaged_bits, (summed_timestamp / 4) as u64);
            }
        }

        Self { node_tree }
    }
}

pub enum NodeTree {
    Bit(TreeOrLayer<Bit>),
    Qit(TreeOrLayer<Qit>),
}

pub struct Waveform {
    pub top: Scope,
    /// Femtoseconds per timestep
    pub timescale: u128,

    /// Value change data
    pub forest: Forest,
}

// fn lazy_load()

/// Contains a forest of trees of value change layers.
pub struct Forest {
    allocator: Alloc,
    trees: Mutex<HashMap<VariableId, Arc<LazyModify<NodeTree>>>>,
}

impl Forest {
    pub fn new(allocator: Alloc, layers: impl Iterator<Item = (VariableId, NodeTree)>) -> Self {
        let trees = layers
            .map(|(id, tree)| (id, Arc::new(LazyModify::new(tree))))
            .collect();

        Self {
            allocator,
            trees: Mutex::new(trees),
        }
    }

    /// If the tree is not "mipmapped", this function will mipmap it before returning.
    pub fn retrieve(&self, id: VariableId) -> Arc<LazyModify<NodeTree>> {
        let node_tree = self.trees.lock().unwrap()[&id].clone();
        node_tree.modify(|node_tree| match node_tree {
            NodeTree::Bit(tree) => NodeTree::Bit(tree.into_tree()),
            NodeTree::Qit(tree) => NodeTree::Qit(tree.into_tree()),
        });
        node_tree
    }
}
