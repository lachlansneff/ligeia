#![feature(allocator_api, generic_associated_types, type_alias_impl_trait)]
#![warn(unsafe_op_in_unsafe_fn)]

mod implicit_forest;
pub mod logic;
pub mod waves;
mod logic2;
mod waves2;
mod forest;

pub use self::implicit_forest::ImplicitForest;
pub use self::waves::{Waves, WavesLoader};
