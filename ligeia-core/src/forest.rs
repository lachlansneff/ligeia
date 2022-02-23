use std::{marker::PhantomData, ops::Range};

use crate::{logic2::{DataWidth, Logic}, waves2::Timesteps};

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct DataIndex(usize);

/// The actual data and length of the array are stored in an `EventStorage` instance.
pub struct Event<L: Logic> {
    timespan: Range<Timesteps>,
    start_idx: DataIndex,
    _marker: PhantomData<L>,
}

impl<L: Logic> Event<L> {
    pub fn from_logic(storage: &mut EventStorage<L>, timespan: Range<Timesteps>, logics: impl Iterator<Item = L>) -> Self {
        Event {
            timespan,
            start_idx: storage.push_pack(logics),
            _marker: PhantomData,
        }
    }

    pub fn from_raw(storage: &mut EventStorage<L>, timespan: Range<Timesteps>, data: &[u8]) -> Self {
        Event {
            timespan,
            start_idx: storage.push_raw(data),
            _marker: PhantomData,
        }
    }

    pub fn unpack<'a>(&self, storage: &'a EventStorage<L>) -> impl Iterator<Item = L> + 'a {
        L::unpack(storage.width, &storage.data[self.start_idx.0..self.start_idx.0 + storage.width_bytes])
    }

    pub fn timespan(&self) -> Range<Timesteps> {
        self.timespan.clone()
    }
}

pub struct EventStorage<L> {
    data: Vec<u8>,
    width: DataWidth,
    width_bytes: usize,
    _marker: PhantomData<L>,
}

impl<L: Logic> EventStorage<L> {
    pub fn new(width: DataWidth) -> Self {
        Self {
            data: Vec::new(),
            width,
            width_bytes: L::bytes(width),
            _marker: PhantomData,
        }
    }

    fn push_pack(&mut self, logics: impl Iterator<Item = L>) -> DataIndex {
        let idx = self.data.len();
        self.data.resize(self.data.len() + self.width_bytes, 0);
        L::pack(logics, &mut self.data[idx..]);
        DataIndex(idx)
    }

    /// Returns the start index of the stored data.
    fn push_raw(&mut self, data: &[u8]) -> DataIndex {
        assert_eq!(data.len(), self.width_bytes);

        let idx = self.data.len();
        self.data.extend_from_slice(data);
        DataIndex(idx)
    }
}
