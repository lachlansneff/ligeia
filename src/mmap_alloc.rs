// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{alloc::{AllocError, Allocator, Global, Layout}, collections::HashMap, fs::File, io, ptr::NonNull, sync::{Mutex, atomic::{AtomicU64, Ordering}}, todo};
use mapr::{MmapMut, MmapOptions};

lazy_static::lazy_static! {
    static ref INITIAL_MEM_INFO: Result<sys_info::MemInfo, String> = {
        sys_info::mem_info().map_err(|e| e.to_string())
    };
}

/// A rough estimate of the data allocated so far.
/// The total size of all value change data should be >> size of other allocated data.
static ALLOCATED_SIZE: AtomicU64 = AtomicU64::new(0);

pub struct MmappableAllocator {
    mappings: Mutex<HashMap<NonNull<u8>, (File, MmapMut)>>,
}

impl MmappableAllocator {
    pub fn new() -> Self {
        Self {
            mappings: Mutex::new(HashMap::new()),
        }
    }

    // #[cold]
    // fn grow(&mut self) {
    //     if !Realloc::ENABLED {
    //         panic!("this instance of `VarMmapVec` is not allowed to reallocate");
    //     }

    //     self.cap *= 2;
    //     self.f
    //         .set_len(self.cap as u64)
    //         .expect("failed to extend file");
    //     let mapping = unsafe {
    //         MmapOptions::new()
    //             .len(self.cap)
    //             .map_mut(&self.f)
    //             .expect("failed to create to mapping")
    //     };
    //     self.mapping = mapping;
    // }
}

unsafe impl Allocator for MmappableAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let layout = layout.pad_to_align();

        if let Ok(true) = INITIAL_MEM_INFO
            .as_ref()
            .map(|info|
                layout.size() >= 10_000_000
                && info.avail <= ALLOCATED_SIZE.load(Ordering::Relaxed) * 2
        ) {
            // Looks like we're getting close to the total size available
            // or we want to allocate a pretty large region.
            
            let f = tempfile::tempfile()
                .map_err(|_| AllocError)?; // TODO: log if there's an error
            
            f.set_len(layout.size() as u64); // Will always be aligned to page size, so should be good enough.

            let mut mapping = unsafe {
                MmapOptions::new()
                    .len(layout.size())
                    .map_mut(&f)
                    .map_err(|_| AllocError)? // TODO: log if there's an error
            };

            let ptr = NonNull::new(mapping.as_mut_ptr()).unwrap();

            self.mappings.lock().unwrap().insert(ptr, (f, mapping));

            ALLOCATED_SIZE.fetch_add(layout.size() as u64, Ordering::Relaxed);

            Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
        } else {
            // Either we can't get information about this machine's memory, or we're nowhere close to the limit.

            ALLOCATED_SIZE.fetch_add(layout.size() as u64, Ordering::Relaxed);
            Global.allocate(layout)
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let _ = ALLOCATED_SIZE.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            if layout.size() as u64 > n {
                Some(0)
            } else {
                Some(n - layout.size() as u64)
            }
        });

        if self.mappings.lock().unwrap().remove(&ptr).is_none() {
            Global.deallocate(ptr, layout)
        }
    }

    unsafe fn grow(&self, ptr: NonNull<u8>, old_layout: Layout, new_layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if let Some((f, mapping)) = self.mappings.lock().unwrap().get(&ptr) {
            todo!("growing an mmapped region is not yet implemented")
        } else {
            Global.grow(ptr, old_layout, new_layout)
        }
    }
}
