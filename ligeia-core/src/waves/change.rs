#[cfg(debug_assertions)]
use std::any::TypeId;
use std::{alloc::Allocator, convert::TryInto, mem, num::NonZeroUsize, ptr::NonNull};

use crate::{
    logic::{Logic, LogicSlice, LogicSliceMut},
    waves::{StorageId, Timesteps},
};

pub const CHANGES_PER_BLOCK: usize = 128;

#[derive(Copy, Clone, bytemuck::Pod)]
#[repr(C)]
struct BlockHeader {
    /// The byte offset from this changeblock to the next.
    pub delta_next: Option<NonZeroUsize>,
    /// Less than or equal to `CHANGES_PER_BLOCK`.
    pub len: usize,
}

impl BlockHeader {
    pub fn is_full(&self) -> bool {
        self.len == CHANGES_PER_BLOCK
    }
}

unsafe impl bytemuck::Zeroable for BlockHeader {}

struct StorageInfo {
    first_offset: usize,
    last_offset: usize,
    /// TODO: Special case changes that are less than a byte in size to store more compactly?
    bytes_per_change: NonZeroUsize,
    width: usize,
    count: usize,

    #[cfg(debug_assertions)]
    logic_type_id: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeOffset(usize);

pub struct ChangeListInfo {
    pub total_bytes_used: usize,
    pub total_storages: usize,
}

pub struct ChangeBlockList<A: Allocator> {
    data: Vec<u8, AlignedAlloc<A, { mem::align_of::<BlockHeader>() }>>,
    /// Indexed by storage ID
    storage_infos: Vec<Option<StorageInfo>>,
    timesteps: Vec<Timesteps>,
    current_timestep_index: u32,
}

impl<A: Allocator> ChangeBlockList<A> {
    pub fn new_in(alloc: A) -> Self {
        Self {
            data: Vec::new_in(AlignedAlloc::new(alloc)),
            storage_infos: Vec::new(),
            timesteps: Vec::new(),
            current_timestep_index: 0,
        }
    }

    pub fn add_storage<L: Logic>(&mut self, storage_id: StorageId, width: usize) {
        if self.storage_infos.len() <= storage_id.0 as usize {
            self.storage_infos
                .resize_with(storage_id.0 as usize + 1, || None);
        }

        let bytes_per_change =
            NonZeroUsize::new(mem::size_of::<u32>() + ((width + L::PER_BYTE - 1) / L::PER_BYTE))
                .unwrap();
        //                                              timestamp index type ^

        // This fixes the alignment as long as the alignment of the vector's allocation is _at least_ the alignment of `BlockHeader`.
        let aligned_len = align_up(self.data.len(), mem::align_of::<BlockHeader>());
        self.data.resize(aligned_len, 0);

        let current_offset = self.data.len();

        let header = BlockHeader {
            delta_next: None,
            len: 0,
        };

        self.data.extend_from_slice(bytemuck::bytes_of(&header));
        self.data.resize(
            self.data.len() + (bytes_per_change.get() * CHANGES_PER_BLOCK),
            0,
        );

        self.storage_infos[storage_id.0 as usize] = Some(StorageInfo {
            first_offset: current_offset,
            last_offset: current_offset,
            bytes_per_change,
            width,
            count: 0,

            #[cfg(debug_assertions)]
            logic_type_id: TypeId::of::<L>(),
        });
    }

    pub fn push_timesteps(&mut self, timesteps: Timesteps) {
        self.current_timestep_index = self.timesteps.len().try_into().unwrap();
        self.timesteps.push(timesteps);
    }

    pub unsafe fn push_get<L: Logic>(&mut self, storage_id: StorageId) -> LogicSliceMut<L> {
        let info = self.storage_infos[storage_id.0 as usize].as_mut().unwrap();

        #[cfg(debug_assertions)]
        if info.logic_type_id != TypeId::of::<L>() {
            panic!("The logical unit used to create this storage is not the same one used for `push_get`")
        }

        info.count += 1;
        let current_offset = self.data.len();

        let (block_header_slice, rest) =
            self.data[info.last_offset..].split_at_mut(mem::size_of::<BlockHeader>());
        let block_header: &mut BlockHeader = bytemuck::from_bytes_mut(block_header_slice);

        let (block_header, rest) = if block_header.is_full() {
            // This fixes the alignment as long as the alignment of the vector's allocation is _at least_ the alignment of `BlockHeader`.
            let aligned_len = align_up(current_offset, mem::align_of::<BlockHeader>());

            // At this point, there's at least one block already full.
            block_header.delta_next =
                Some(unsafe { NonZeroUsize::new_unchecked(aligned_len - info.last_offset) });
            info.last_offset = current_offset;

            drop(block_header);
            drop(rest);

            self.data.resize(aligned_len, 0);

            let new_header = BlockHeader {
                delta_next: None,
                len: 0,
            };

            self.data.extend_from_slice(bytemuck::bytes_of(&new_header));
            self.data.resize(
                self.data.len() + info.bytes_per_change.get() * CHANGES_PER_BLOCK,
                0,
            );

            let (block_header_slice, rest) =
                self.data[aligned_len..].split_at_mut(mem::size_of::<BlockHeader>());
            let block_header: &mut BlockHeader = bytemuck::from_bytes_mut(block_header_slice);
            (block_header, rest)
        } else {
            (block_header, rest)
        };

        let change_offset = block_header.len * info.bytes_per_change.get();

        let (header_slice, data_slice) = rest
            [change_offset..change_offset + info.bytes_per_change.get()]
            .split_at_mut(mem::size_of::<u32>());
        header_slice.copy_from_slice(&self.current_timestep_index.to_le_bytes());

        block_header.len += 1;

        unsafe { LogicSliceMut::new(info.width, NonNull::new_unchecked(data_slice.as_mut_ptr())) }
    }

    pub fn count_of(&self, storage_id: StorageId) -> usize {
        self.storage_infos[storage_id.0 as usize]
            .as_ref()
            .expect("storage not added yet")
            .count
    }

    pub fn info(&self) -> ChangeListInfo {
        ChangeListInfo {
            total_bytes_used: self.data.capacity(),
            total_storages: self.storage_infos.len(),
        }
    }

    pub fn iter_storage(&self, storage_id: StorageId) -> StorageIter {
        let info = self.storage_infos[storage_id.0 as usize]
            .as_ref()
            .expect("storage not added yet");

        StorageIter {
            block_and: &self.data[info.first_offset..],
            block_offset: info.first_offset,
            remaining: info.count,
            index_in_block: 0,
            bytes_per_change: info.bytes_per_change.get(),
        }
    }

    /// This isn't "unsafe" exactly, but if you put in a wrong offset, you'll get garbage out.
    pub unsafe fn get_change<L: Logic>(
        &self,
        offset: ChangeOffset,
        width: usize,
    ) -> (Timesteps, LogicSlice<L>) {
        let offset = offset.0;
        let timestep_index = u32::from_le_bytes(
            self.data[offset..offset + mem::size_of::<u32>()]
                .try_into()
                .unwrap(),
        );
        let ptr = NonNull::from(&self.data[offset + mem::size_of::<u32>()]);
        let timestamp = self.timesteps[timestep_index as usize];

        (timestamp, unsafe { LogicSlice::new(width, ptr) })
    }

    /// This isn't "unsafe" exactly, but if you put in a wrong offset, you'll get garbage out.
    pub unsafe fn get_change_mut<L: Logic>(
        &mut self,
        offset: ChangeOffset,
        width: usize,
    ) -> (Timesteps, LogicSliceMut<L>) {
        let offset = offset.0;
        let timestep_index = u32::from_le_bytes(
            self.data[offset..offset + mem::size_of::<u32>()]
                .try_into()
                .unwrap(),
        );
        let ptr = NonNull::from(&mut self.data[offset + mem::size_of::<u32>()]);
        let timestamp = self.timesteps[timestep_index as usize];

        (timestamp, unsafe { LogicSliceMut::new(width, ptr) })
    }

    pub unsafe fn get_two_changes<L: Logic>(
        &mut self,
        lhs: ChangeOffset,
        rhs: ChangeOffset,
        width: usize,
    ) -> ((Timesteps, LogicSliceMut<L>), (Timesteps, LogicSlice<L>)) {
        let (h1, ptr1) = {
            let offset = lhs.0;
            let timestep_index = u32::from_le_bytes(
                self.data[offset..offset + mem::size_of::<u32>()]
                    .try_into()
                    .unwrap(),
            );
            let ptr = NonNull::from(&mut self.data[offset + mem::size_of::<u32>()]);
            let timestamp = self.timesteps[timestep_index as usize];
            (timestamp, ptr)
        };

        let (h2, ptr2) = {
            let offset = rhs.0;
            let timestep_index = u32::from_le_bytes(
                self.data[offset..offset + mem::size_of::<u32>()]
                    .try_into()
                    .unwrap(),
            );
            let ptr = NonNull::from(&self.data[offset + mem::size_of::<u32>()]);
            let timestamp = self.timesteps[timestep_index as usize];
            (timestamp, ptr)
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
    index_in_block: usize,
    bytes_per_change: usize,
}

impl<'a> Iterator for StorageIter<'a> {
    /// Returns the offset of the value change header.
    type Item = ChangeOffset;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.index_in_block == CHANGES_PER_BLOCK {
                let header: &BlockHeader =
                    bytemuck::from_bytes(&self.block_and[..mem::size_of::<BlockHeader>()]);
                let delta_offset = header.delta_next.unwrap().get();
                self.block_offset += delta_offset;
                self.block_and = &self.block_and[delta_offset..];
                self.index_in_block = 0;
            }

            let offset = self.block_offset
                + mem::size_of::<BlockHeader>()
                + (self.bytes_per_change * self.index_in_block);

            self.index_in_block += 1;

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

/// Align downwards. Returns the greatest x with alignment `align`
/// so that x <= addr. The alignment must be a power of 2.
fn align_down(addr: usize, align: usize) -> usize {
    if align.is_power_of_two() {
        addr & !(align - 1)
    } else if align == 0 {
        addr
    } else {
        panic!("`align` must be a power of 2");
    }
}

/// Align upwards. Returns the smallest x with alignment `align`
/// so that x >= addr. The alignment must be a power of 2.
fn align_up(addr: usize, align: usize) -> usize {
    align_down(addr + align - 1, align)
}

struct AlignedAlloc<A: Allocator, const ALIGN: usize> {
    alloc: A,
}

impl<A: Allocator, const ALIGN: usize> AlignedAlloc<A, ALIGN> {
    fn new(alloc: A) -> Self {
        assert!(ALIGN.is_power_of_two(), "ALIGN is not power of two");
        Self { alloc }
    }
}

unsafe impl<A: Allocator, const ALIGN: usize> Allocator for AlignedAlloc<A, ALIGN> {
    fn allocate(
        &self,
        layout: std::alloc::Layout,
    ) -> Result<NonNull<[u8]>, std::alloc::AllocError> {
        self.alloc.allocate(layout.align_to(ALIGN).unwrap())
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: std::alloc::Layout) {
        unsafe { self.alloc.deallocate(ptr, layout) }
    }

    fn allocate_zeroed(
        &self,
        layout: std::alloc::Layout,
    ) -> Result<NonNull<[u8]>, std::alloc::AllocError> {
        self.alloc.allocate_zeroed(layout.align_to(ALIGN).unwrap())
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: std::alloc::Layout,
        new_layout: std::alloc::Layout,
    ) -> Result<NonNull<[u8]>, std::alloc::AllocError> {
        unsafe {
            self.alloc
                .grow(ptr, old_layout, new_layout.align_to(ALIGN).unwrap())
        }
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: std::alloc::Layout,
        new_layout: std::alloc::Layout,
    ) -> Result<NonNull<[u8]>, std::alloc::AllocError> {
        unsafe {
            self.alloc
                .grow_zeroed(ptr, old_layout, new_layout.align_to(ALIGN).unwrap())
        }
    }

    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: std::alloc::Layout,
        new_layout: std::alloc::Layout,
    ) -> Result<NonNull<[u8]>, std::alloc::AllocError> {
        unsafe {
            self.alloc
                .shrink(ptr, old_layout, new_layout.align_to(ALIGN).unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::alloc::Global;

    use crate::logic;

    use super::*;

    #[test]
    fn change_list_push() {
        let mut list = ChangeBlockList::new_in(Global);
        list.add_storage::<logic::Four>(StorageId(0), 3);
        {
            let mut slice = unsafe { list.push_get::<logic::Four>(StorageId(0)) };
            for i in 0..3 {
                slice.set(i, logic::Four::One);
            }
        }

        let mut iter = list.iter_storage(StorageId(0));

        let offset = iter.next().unwrap();
        assert_eq!(iter.next(), None);

        let (header, slice) = unsafe { list.get_change::<logic::Four>(offset, 3) };

        assert_eq!(header, TimeIdx(42));
        for i in 0..3 {
            assert_eq!(slice.get(i), logic::Four::One);
        }
    }
}
