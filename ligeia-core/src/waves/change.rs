use std::{
    alloc::Allocator, convert::TryInto, marker::PhantomData, mem, num::NonZeroUsize, ptr::NonNull,
    slice,
};

use crate::{
    logic::{Logic, LogicSlice, LogicSliceMut},
    waves::StorageId,
};

pub const CHANGES_PER_BLOCK: usize = 16;

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
    /// The byte offset from this changeblock to the next.
    pub delta_next: Option<NonZeroUsize>,
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
    first_offset: usize,
    last_offset: usize,
    /// TODO: Special case changes that are less than a byte in size to store more compactly?
    bytes_per_change: NonZeroUsize,
    count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ChangeOffset(usize);

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

    pub fn add_storage(&mut self, storage_id: StorageId, bytes: NonZeroUsize) {
        if self.storage_infos.len() <= storage_id.0 as usize {
            self.storage_infos
                .resize_with(storage_id.0 as usize + 1, || None);
        }

        let current_offset = self.data.len();
        let bytes_per_change = unsafe { NonZeroUsize::new_unchecked(mem::size_of::<ChangeBlockHeader>() * bytes.get()) };

        let header = ChangeBlockHeader {
            delta_next: None,
            len: 0,
        };

        self.data.extend_from_slice(bytemuck::bytes_of(&header));
        self.data
            .resize(self.data.len() + (bytes_per_change.get() * CHANGES_PER_BLOCK), 0);

        self.storage_infos[storage_id.0 as usize] = Some(StorageInfo {
            first_offset: current_offset,
            last_offset: current_offset,
            bytes_per_change,
            count: 0,
        });
    }

    pub unsafe fn push_change(&mut self, storage_id: StorageId, data: *const u8) {
        let info = self.storage_infos[storage_id.0 as usize].as_mut().unwrap();
        info.count += 1;
        let current_offset = self.data.len();

        let (block_header_slice, rest) = self.data[info.last_offset..].split_at_mut(mem::size_of::<ChangeBlockHeader>());
        let block_header: &mut ChangeBlockHeader = bytemuck::from_bytes_mut(block_header_slice);
        
        if !block_header.is_full() {
            let change_offset = block_header.len * info.bytes_per_change.get();
            let data_len = info.bytes_per_change.get() - mem::size_of::<ChangeBlockHeader>();

            rest[change_offset..change_offset + data_len]
                .copy_from_slice(unsafe { slice::from_raw_parts(data, data_len) });
            
            block_header.len += 1;
        } else {
            // At this point, there's at least one block already full.
            block_header.delta_next = Some(unsafe { NonZeroUsize::new_unchecked(current_offset - info.last_offset) });
            info.last_offset = current_offset;

            drop(block_header);
            drop(rest);

            let new_header = ChangeBlockHeader {
                delta_next: None,
                len: 0,
            };

            self.data.extend_from_slice(bytemuck::bytes_of(&new_header));
            self.data
                .resize(self.data.len() + info.bytes_per_change.get() * CHANGES_PER_BLOCK, 0);
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

        let header: &ChangeBlockHeader = bytemuck::from_bytes(
            &self.data[info.first_offset
                ..info.first_offset + mem::size_of::<ChangeBlockHeader>()],
        );

        StorageIter {
            block_and: &self.data[info.first_offset..],
            block_offset: info.first_offset,
            remaining: info.count,
            remaining_in_block: header.len,
            bytes_per_change: info.bytes_per_change.get(),
        }
    }

    /// This isn't "unsafe" exactly, but if you put in a wrong offset, you'll get garbage out.
    pub unsafe fn get_change<L: Logic>(
        &self,
        offset: ChangeOffset,
        width: usize,
    ) -> (ChangeHeader, LogicSlice<L>) {
        let offset = offset.0;
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
        let offset = offset.0;
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
            let offset = lhs.0;
            let header =
                *bytemuck::from_bytes(&self.data[offset..offset + mem::size_of::<ChangeHeader>()]);
            let ptr = NonNull::from(&mut self.data[offset + mem::size_of::<ChangeHeader>()]);
            (header, ptr)
        };

        let (h2, ptr2) = {
            let offset = rhs.0;
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
    block_and: &'a [u8],
    block_offset: usize,
    remaining: usize,
    remaining_in_block: usize,
    bytes_per_change: usize,
}

impl<'a> Iterator for StorageIter<'a> {
    /// Returns the offset of the value change header.
    type Item = ChangeOffset;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.remaining_in_block == 0 {
                let header: &ChangeBlockHeader = bytemuck::from_bytes(&self.block_and[..mem::size_of::<ChangeBlockHeader>()]);
                let delta_offset = header.delta_next.unwrap().get();
                self.block_offset += delta_offset;
                self.block_and = &self.block_and[delta_offset..];
                self.remaining_in_block = header.len;
            }

            let offset = self.block_offset + mem::size_of::<ChangeBlockHeader>() + (self.bytes_per_change * (CHANGES_PER_BLOCK - self.remaining_in_block));

            Some(ChangeOffset(offset))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for StorageIter<'_> {}
