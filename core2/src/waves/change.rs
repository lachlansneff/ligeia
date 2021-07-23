use std::{alloc::Allocator, convert::TryInto, marker::PhantomData, mem, num::NonZeroUsize, slice};

pub const CHANGES_PER_BLOCK: usize = 16;

fn size_of_changes(bytes_per_change: usize) -> usize {
    (8 + bytes_per_change) * CHANGES_PER_BLOCK
}

fn block_offset_to_change_offset(block_header_offset: usize, bytes_per_change: usize, index: usize) -> usize {
    block_header_offset + mem::size_of::<ChangeBlockHeader>() + ((8 + bytes_per_change) * index)
}

pub struct ChangeHeader {
    pub ts: u64, // timestamp of timesteps
}

impl From<[u8; 8]> for ChangeHeader {
    fn from(b: [u8; 8]) -> Self {
        Self {
            ts: u64::from_le_bytes(b),
        }
    }
}

impl From<ChangeHeader> for [u8; 8] {
    fn from(h: ChangeHeader) -> Self {
        h.ts.to_le_bytes()
    }
}

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

impl From<[u8; mem::size_of::<ChangeBlockHeader>()]> for ChangeBlockHeader {
    fn from(b: [u8; mem::size_of::<ChangeBlockHeader>()]) -> Self {
        Self {
            delta_previous: NonZeroUsize::new(usize::from_le_bytes(b[..mem::size_of::<usize>()].try_into().unwrap())),
            len: usize::from_le_bytes(b[mem::size_of::<usize>()..].try_into().unwrap())
        }
    }
}

impl From<ChangeBlockHeader> for [u8; mem::size_of::<ChangeBlockHeader>()] {
    fn from(h: ChangeBlockHeader) -> Self {
        let mut b = [0; mem::size_of::<ChangeBlockHeader>()];
        b[..mem::size_of::<usize>()].copy_from_slice(&h.delta_previous.map_or(0, |n| n.get()).to_le_bytes());
        b[mem::size_of::<usize>()..].copy_from_slice(&h.len.to_le_bytes());
        b
    }
}

struct StorageInfo {
    last_offset: NonZeroUsize,
    /// TODO: Special case changes that are less than a byte in size to store more compactly?
    bytes_per_change: usize,
    count: usize,
}

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

    pub fn add_storage(&mut self, storage_id: u32, bytes_per_change: usize) {
        if self.storage_infos.len() <= storage_id as usize {
            self.storage_infos.resize_with(storage_id as usize + 1, || None);
        }

        let current_offset = self.data.len();

        let header: [u8; mem::size_of::<ChangeBlockHeader>()] = ChangeBlockHeader {
            delta_previous: None,
            len: 0,
        }.into();

        self.data.extend_from_slice(&header);
        self.data.resize(self.data.len() + size_of_changes(bytes_per_change), 0);

        self.storage_infos[storage_id as usize] = Some(StorageInfo {
            last_offset: current_offset.try_into().unwrap(),
            bytes_per_change,
            count: 0,
        });
    }

    pub unsafe fn push_change(&mut self, storage_id: u32, data: *const u8) {
        let info = self.storage_infos[storage_id as usize].as_mut().unwrap();
        let block_header: ChangeBlockHeader = unsafe {
            (&self.data[info.last_offset.get()] as *const u8 as *const [u8; mem::size_of::<ChangeBlockHeader>()]).read().into()
        };

        info.count += 1;

        if !block_header.is_full() {
            let change_offset = block_offset_to_change_offset(info.last_offset.get(), info.bytes_per_change, block_header.len);
            self.data[change_offset..change_offset + info.bytes_per_change].copy_from_slice(unsafe {
                slice::from_raw_parts(data, info.bytes_per_change)
            });
            
            let block_header: [u8; mem::size_of::<ChangeBlockHeader>()] = ChangeBlockHeader {
                len: block_header.len + 1,
                ..block_header
            }.into();

            unsafe {
                (&mut self.data[info.last_offset.get()] as *mut u8 as *mut [u8; mem::size_of::<ChangeBlockHeader>()]).write(block_header);
            }
        } else {
            let new_offset = self.data.len();

            let header: [u8; mem::size_of::<ChangeBlockHeader>()] = ChangeBlockHeader {
                delta_previous: Some(unsafe { NonZeroUsize::new_unchecked(new_offset - info.last_offset.get()) }),
                len: 0,
            }.into();
            
            self.data.extend_from_slice(&header);
            self.data.resize(self.data.len() + size_of_changes(info.bytes_per_change), 0);

            info.last_offset = unsafe { NonZeroUsize::new_unchecked(new_offset) };
        }
    }

    pub fn count_of(&self, storage_id: u32) -> usize {
        self.storage_infos[storage_id as usize].as_ref().expect("storage not added yet").count
    }

    pub fn iter_storage(&self, storage_id: u32) -> StorageIter {
        let info = self.storage_infos[storage_id as usize].as_ref().expect("storage not added yet");

        let block_header: ChangeBlockHeader = unsafe {
            (&self.data[info.last_offset.get()] as *const u8 as *const [u8; mem::size_of::<ChangeBlockHeader>()]).read().into()
        };

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
    pub fn get_change(&self, offset: ChangeOffset) -> (ChangeHeader, *const u8) {
        let offset = offset.0;
        let b: [u8; 8] = self.data[offset..offset + 8].try_into().unwrap();
        let header = b.into();

        (header, &self.data[offset + 8])
    }
}

pub struct ChangeOffset(usize);

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
                let header: ChangeBlockHeader = (self.block_pointer as *const [u8; mem::size_of::<ChangeBlockHeader>()]).read().into();
                if let Some(delta) = header.delta_previous {
                    self.block_pointer = self.block_pointer.sub(delta.get());
                    let header: ChangeBlockHeader = (self.block_pointer as *const [u8; mem::size_of::<ChangeBlockHeader>()]).read().into();
                    self.remaining_in_block = header.len;
                } else {
                    return None;
                }
            }
        }

        let change_offset = block_offset_to_change_offset(0, self.bytes_per_change, CHANGES_PER_BLOCK - self.remaining_in_block);
        unsafe {
            let header_offset = self.block_pointer.add(change_offset).offset_from(self.start_ptr) as usize;
            Some(ChangeOffset(header_offset))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for StorageIter<'_> {}
