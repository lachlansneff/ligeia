use std::{
    alloc::Allocator, convert::TryInto, marker::PhantomData, mem, num::NonZeroUsize, ptr::NonNull,
    slice,
};

use crate::{
    logic::{Logic, LogicSlice, LogicSliceMut},
    waves::StorageId,
};

pub const CHANGES_PER_BLOCK: usize = 16;

fn size_of_changes(bytes_per_change: usize) -> usize {
    (mem::size_of::<ChangeHeader>() + bytes_per_change) * CHANGES_PER_BLOCK
}

fn block_offset_to_change_offset(
    block_header_offset: usize,
    bytes_per_change: usize,
    index: usize,
) -> usize {
    block_header_offset
        + mem::size_of::<ChangeBlockHeader>()
        + ((mem::size_of::<ChangeHeader>() + bytes_per_change) * index)
}

#[derive(Copy, Clone, bytemuck::Pod)]
#[repr(C)]
pub struct ChangeHeader {
    pub ts: u64, // timestamp of timesteps
}

unsafe impl bytemuck::Zeroable for ChangeHeader {}

#[derive(Copy, Clone, bytemuck::Pod)]
#[repr(C)]
pub struct ChangeBlockHeader {
    /// The byte offset from this changeblock to the previous one.
    pub delta_previous: Option<NonZeroUsize>,
    /// Less than or equal to `CHANGES_PER_BLOCK`.
    pub len: usize,
}

impl ChangeBlockHeader {
    pub fn is_full(&self) -> bool {
        self.len == CHANGES_PER_BLOCK
    }
}

unsafe impl bytemuck::Zeroable for ChangeBlockHeader {}

struct StorageInfo {
    last_offset: NonZeroUsize,
    /// TODO: Special case changes that are less than a byte in size to store more compactly?
    bytes_per_change: usize,
    count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ChangeOffset(NonZeroUsize);

pub struct ChangeBlockList<A: Allocator> {
    data: Vec<u8, A>,
    /// Indexed by storage ID
    storage_infos: Vec<Option<StorageInfo>>,
}

impl<A: Allocator> ChangeBlockList<A> {
    pub fn new_in(alloc: A) -> Self {
        Self {
            data: Vec::new_in(alloc),
            storage_infos: Vec::new(),
        }
    }

    pub fn add_storage(&mut self, storage_id: StorageId, bytes_per_change: usize) {
        if self.storage_infos.len() <= storage_id.0 as usize {
            self.storage_infos
                .resize_with(storage_id.0 as usize + 1, || None);
        }

        let current_offset = self.data.len();

        let header = ChangeBlockHeader {
            delta_previous: None,
            len: 0,
        };

        self.data.extend_from_slice(bytemuck::bytes_of(&header));
        self.data
            .resize(self.data.len() + size_of_changes(bytes_per_change), 0);

        self.storage_infos[storage_id.0 as usize] = Some(StorageInfo {
            last_offset: current_offset.try_into().unwrap(),
            bytes_per_change,
            count: 0,
        });
    }

    pub unsafe fn push_change(&mut self, storage_id: StorageId, data: *const u8) {
        let info = self.storage_infos[storage_id.0 as usize].as_mut().unwrap();

        let block_header: ChangeBlockHeader = *bytemuck::from_bytes(
            &self.data[info.last_offset.get()
                ..info.last_offset.get() + mem::size_of::<ChangeBlockHeader>()],
        );

        info.count += 1;

        if !block_header.is_full() {
            let change_offset = block_offset_to_change_offset(
                info.last_offset.get(),
                info.bytes_per_change,
                block_header.len,
            );
            self.data[change_offset..change_offset + info.bytes_per_change]
                .copy_from_slice(unsafe { slice::from_raw_parts(data, info.bytes_per_change) });
            let block_header: &mut ChangeBlockHeader = bytemuck::from_bytes_mut(
                &mut self.data[info.last_offset.get()
                    ..info.last_offset.get() + mem::size_of::<ChangeBlockHeader>()],
            );

            block_header.len += 1;
        } else {
            let new_offset = self.data.len();

            let header = ChangeBlockHeader {
                delta_previous: Some(unsafe {
                    NonZeroUsize::new_unchecked(new_offset - info.last_offset.get())
                }),
                len: 0,
            };

            self.data.extend_from_slice(bytemuck::bytes_of(&header));
            self.data
                .resize(self.data.len() + size_of_changes(info.bytes_per_change), 0);

            info.last_offset = unsafe { NonZeroUsize::new_unchecked(new_offset) };
        }
    }

    pub fn count_of(&self, storage_id: StorageId) -> usize {
        self.storage_infos[storage_id.0 as usize]
            .as_ref()
            .expect("storage not added yet")
            .count
    }

    pub fn iter_storage(&self, storage_id: StorageId) -> StorageIter {
        let info = self.storage_infos[storage_id.0 as usize]
            .as_ref()
            .expect("storage not added yet");

        let block_header: &ChangeBlockHeader = bytemuck::from_bytes(
            &self.data[info.last_offset.get()
                ..info.last_offset.get() + mem::size_of::<ChangeBlockHeader>()],
        );

        StorageIter {
            start_ptr: self.data.as_ptr(),
            block_pointer: &self.data[info.last_offset.get()],
            remaining: info.count,
            remaining_in_block: block_header.len,
            bytes_per_change: info.bytes_per_change,
            _marker: PhantomData,
        }
    }

    /// This isn't "unsafe" exactly, but if you put in a wrong offset, you'll get garbage out.
    pub unsafe fn get_change<L: Logic>(
        &self,
        offset: ChangeOffset,
        width: usize,
    ) -> (ChangeHeader, LogicSlice<L>) {
        let offset = offset.0.get();
        let header =
            *bytemuck::from_bytes(&self.data[offset..offset + mem::size_of::<ChangeHeader>()]);
        let ptr = NonNull::from(&self.data[offset + mem::size_of::<ChangeHeader>()]);

        (header, unsafe { LogicSlice::new(width, ptr) })
    }

    /// This isn't "unsafe" exactly, but if you put in a wrong offset, you'll get garbage out.
    pub unsafe fn get_change_mut<L: Logic>(
        &mut self,
        offset: ChangeOffset,
        width: usize,
    ) -> (ChangeHeader, LogicSliceMut<L>) {
        let offset = offset.0.get();
        let header =
            *bytemuck::from_bytes(&self.data[offset..offset + mem::size_of::<ChangeHeader>()]);
        let ptr = NonNull::from(&mut self.data[offset + mem::size_of::<ChangeHeader>()]);

        (header, unsafe { LogicSliceMut::new(width, ptr) })
    }

    pub unsafe fn get_two_changes<L: Logic>(
        &mut self,
        lhs: ChangeOffset,
        rhs: ChangeOffset,
        width: usize,
    ) -> (
        (ChangeHeader, LogicSliceMut<L>),
        (ChangeHeader, LogicSlice<L>),
    ) {
        let (h1, ptr1) = {
            let offset = lhs.0.get();
            let header =
                *bytemuck::from_bytes(&self.data[offset..offset + mem::size_of::<ChangeHeader>()]);
            let ptr = NonNull::from(&mut self.data[offset + mem::size_of::<ChangeHeader>()]);
            (header, ptr)
        };

        let (h2, ptr2) = {
            let offset = rhs.0.get();
            let header =
                *bytemuck::from_bytes(&self.data[offset..offset + mem::size_of::<ChangeHeader>()]);
            let ptr = NonNull::from(&self.data[offset + mem::size_of::<ChangeHeader>()]);
            (header, ptr)
        };

        unsafe {
            (
                (h1, LogicSliceMut::new(width, ptr1)),
                (h2, LogicSlice::new(width, ptr2)),
            )
        }
    }
}

pub struct StorageIter<'a> {
    start_ptr: *const u8,
    block_pointer: *const u8,
    remaining: usize,
    remaining_in_block: usize,
    bytes_per_change: usize,
    _marker: PhantomData<&'a [u8]>,
}

impl<'a> Iterator for StorageIter<'a> {
    /// Returns the offset of the value change header.
    type Item = ChangeOffset;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_in_block == 0 {
            unsafe {
                let header: &ChangeBlockHeader = bytemuck::from_bytes(
                    &*(self.block_pointer as *const [u8; mem::size_of::<ChangeBlockHeader>()]),
                );
                if let Some(delta) = header.delta_previous {
                    self.block_pointer = self.block_pointer.sub(delta.get());
                    let header: &ChangeBlockHeader = bytemuck::from_bytes(
                        &*(self.block_pointer as *const [u8; mem::size_of::<ChangeBlockHeader>()]),
                    );
                    self.remaining_in_block = header.len;
                } else {
                    return None;
                }
            }
        }

        let change_offset = block_offset_to_change_offset(
            0,
            self.bytes_per_change,
            CHANGES_PER_BLOCK - self.remaining_in_block,
        );
        unsafe {
            let header_offset = NonZeroUsize::new(
                self.block_pointer
                    .add(change_offset)
                    .offset_from(self.start_ptr) as usize,
            )
            .unwrap();
            Some(ChangeOffset(header_offset))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for StorageIter<'_> {}
