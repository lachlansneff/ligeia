// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use mapr::{MmapMut, MmapOptions};
use std::{
    alloc::{AllocError, Allocator, Global, Layout},
    collections::HashMap,
    fs::File,
    ptr::{self, NonNull},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

lazy_static::lazy_static! {
    static ref INITIAL_AVAILABLE_MEMORY: Option<u64> = {
        effective_limits::memory_limit().ok()
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

unsafe impl Send for MmappableAllocator {}

impl MmappableAllocator {
    pub fn new() -> Self {
        Self {
            mappings: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn rough_total_usage() -> usize {
        ALLOCATED_SIZE.load(Ordering::Relaxed)
    }

    fn allocate_mmap(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let f = tempfile::tempfile().map_err(|_| AllocError)?; // TODO: log if there's an error

        f.set_len(layout.size() as u64) // Will always be aligned to page size, so should be good enough.
            .map_err(|_| AllocError)?; // TODO: log if there's an error

        let mapping = unsafe {
            MmapOptions::new()
                .len(layout.size())
                .map_mut(&f)
                .map_err(|_| AllocError)? // TODO: log if there's an error
        };
        let ptr = NonNull::from(&mapping[..]);

        self.mappings
            .lock()
            .unwrap()
            .insert(ptr.as_non_null_ptr(), (f, mapping));

        Ok(ptr)
    }
}

fn should_mmap(size: usize) -> bool {
    INITIAL_AVAILABLE_MEMORY.map(|remaining| {
        size >= 10_000_000 || remaining <= ALLOCATED_SIZE.load(Ordering::Relaxed) as u64 * 2
    }) == Some(true)
}

unsafe impl Allocator for MmappableAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let layout = layout.pad_to_align();

        let ptr = if should_mmap(layout.size()) {
            // Looks like we're getting close to the total size available
            // or we want to allocate a pretty large region.

            self.allocate_mmap(layout)?
        } else {
            // Either we can't get information about this machine's memory, or we're nowhere close to the limit.

            let ptr = Global.allocate(layout)?;
            ALLOCATED_SIZE.fetch_add(ptr.len(), Ordering::Relaxed);
            ptr
        };

        Ok(ptr)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if let Some((_, _mapping)) = self.mappings.lock().unwrap().remove(&ptr) {
            return;
        }

        Global.deallocate(ptr, layout);
        ALLOCATED_SIZE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        {
            let mut mappings = self.mappings.lock().unwrap();

            if let Some((f, _mapping)) = mappings.remove(&ptr) {
                f.set_len(new_layout.size() as u64)
                    .map_err(|_| AllocError)?; // TODO: log if there's an error

                let mapping = MmapOptions::new()
                    .len(new_layout.size())
                    .map_mut(&f)
                    .map_err(|_| AllocError)?; // TODO: log if there's an error
                let ptr = NonNull::from(&mapping[..]);

                mappings.insert(ptr.as_non_null_ptr(), (f, mapping));

                return Ok(ptr);
            }
        }

        if should_mmap(new_layout.size()) {
            ALLOCATED_SIZE.fetch_sub(old_layout.size(), Ordering::Relaxed);
            // Switch this to an mmapped region.
            let new_layout = new_layout.pad_to_align();
            let new_region = self.allocate_mmap(new_layout)?;

            ptr::copy_nonoverlapping(ptr.as_ptr(), new_region.as_mut_ptr(), old_layout.size());
            Global.deallocate(ptr, old_layout);

            Ok(new_region)
        } else {
            ALLOCATED_SIZE.fetch_add(new_layout.size() - old_layout.size(), Ordering::Relaxed);
            Global.grow(ptr, old_layout, new_layout)
        }
    }
}
