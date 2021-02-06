// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{alloc::{AllocError, Allocator, Global, Layout}, collections::HashMap, fs::File, io, ptr::{self, NonNull}, sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}}, todo};
use mapr::{MmapMut, MmapOptions};

lazy_static::lazy_static! {
    static ref INITIAL_MEM_INFO: Option<sys_info::MemInfo> = {
        sys_info::mem_info().ok()
    };
}

/// A rough estimate of the data allocated so far.
/// The total size of all value change data should be >> size of other allocated data.
static ALLOCATED_SIZE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub struct MmappableAllocator {
    // TODO: Replace this with just Mutex, and have the Arc be external
    // when Allocator is implemented for Arc<A>
    mappings: Arc<Mutex<HashMap<NonNull<u8>, (File, MmapMut)>>>,
}

impl MmappableAllocator {
    pub fn new() -> Self {
        Self {
            mappings: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn rough_total_usage(&self) -> usize {
        ALLOCATED_SIZE.load(Ordering::Relaxed)
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

    fn allocate_mmap(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let f = tempfile::tempfile()
            .map_err(|_| AllocError)?; // TODO: log if there's an error
        
        f.set_len(layout.size() as u64) // Will always be aligned to page size, so should be good enough.
            .map_err(|_| AllocError)?; // TODO: log if there's an error

        let mut mapping = unsafe {
            MmapOptions::new()
                .len(layout.size())
                .map_mut(&f)
                .map_err(|_| AllocError)? // TODO: log if there's an error
        };

        let ptr = NonNull::new(mapping.as_mut_ptr()).unwrap();

        self.mappings.lock().unwrap().insert(ptr, (f, mapping));

        ALLOCATED_SIZE.fetch_add(layout.size(), Ordering::Relaxed);

        Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
    }
}

fn should_mmap(size: usize) -> bool {
    INITIAL_MEM_INFO
        .as_ref()
        .map(|info|
            size >= 20_000_000
            && info.avail <= ALLOCATED_SIZE.load(Ordering::Relaxed) as u64 * 2
    ) == Some(true)
}

unsafe impl Allocator for MmappableAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let layout = layout.pad_to_align();

        if should_mmap(layout.size()) {
            // Looks like we're getting close to the total size available
            // or we want to allocate a pretty large region.
            
            self.allocate_mmap(layout)
        } else {
            // Either we can't get information about this machine's memory, or we're nowhere close to the limit.

            ALLOCATED_SIZE.fetch_add(layout.size(), Ordering::Relaxed);
            Global.allocate(layout)
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let _ = ALLOCATED_SIZE.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            if layout.size() > n {
                Some(0)
            } else {
                Some(n - layout.size())
            }
        });

        if self.mappings.lock().unwrap().remove(&ptr).is_none() {
            Global.deallocate(ptr, layout)
        }
    }

    unsafe fn grow(&self, ptr: NonNull<u8>, old_layout: Layout, new_layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if let Some((f, mapping)) = self.mappings.lock().unwrap().get_mut(&ptr) {
            f
                .set_len(new_layout.size() as u64)
                .map_err(|_| AllocError)?; // TODO: log if there's an error
            
            *mapping = MmapOptions::new()
                .len(new_layout.size())
                .map_mut(&f)
                .map_err(|_| AllocError)?; // TODO: log if there's an error

            Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
        } else {
            if should_mmap(new_layout.size()) {
                // Switch this to an mmapped region.
                let new_layout = new_layout.pad_to_align();
                let new_region = self.allocate_mmap(new_layout)?;
                
                ptr::copy_nonoverlapping(ptr.as_ptr(), new_region.as_mut_ptr(), old_layout.size());
                Global.deallocate(ptr, old_layout);

                Ok(new_region)
            } else {
                Global.grow(ptr, old_layout, new_layout)
            }
        }
    }
}
