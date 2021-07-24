#![feature(allocator_api)]
#![warn(unsafe_op_in_unsafe_fn)]

mod implicit_forest;
pub mod logic;
pub mod waves;

pub use self::implicit_forest::ImplicitForest;
pub use self::waves::{Waves, WavesLoader};
