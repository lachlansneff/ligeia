#![feature(
    allocator_api,
)]

#![warn(unsafe_op_in_unsafe_fn)]

mod implicit_forest;
mod unit;
pub mod waves;

// pub use self::unit::{Unit, TwoLogic, FourLogic, NineLogic, UnitArray, UnitSlice, UnitIter, Mut};
pub use self::implicit_forest::ImplicitForest;