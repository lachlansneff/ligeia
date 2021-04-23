#![feature(
    allocator_api,
    alloc_layout_extra,
    slice_ptr_len,
    slice_ptr_get,
    slice_range,
    maybe_uninit_ref
)]

mod lazy;
#[cfg(feature = "mmap-alloc")]
pub mod mmap_alloc;
mod progress;
mod unsized_types;
pub mod waveform;

pub use progress::Progress;
pub use unsized_types::{
    Bit, BitSlice, KnownUnsizedVec, KnownUnsizedVecIter, KnownUnsizedVecIterMut, Qit,
    ValueChangeNode,
};
// pub use waveform::{Forest, Waveform, WaveformLoader, NodeTree, TreeOrLayer, Tree, LoadError, Scope, VariableId, VariableInfo, Variable};
