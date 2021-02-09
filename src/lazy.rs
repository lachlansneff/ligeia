// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{cell::UnsafeCell, mem::{self, MaybeUninit}, ops::Deref, sync::{Mutex, atomic::{AtomicBool, Ordering}}};

pub struct LazyModify<T> {
    modified: AtomicBool,
    lock: Mutex<()>,
    value: UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T> Sync for LazyModify<T> {}
unsafe impl<T> Send for LazyModify<T> {}

impl<T> LazyModify<T> {
    pub fn new(init: T) -> Self {
        Self {
            modified: AtomicBool::new(false),
            lock: Mutex::new(()),
            value: UnsafeCell::new(MaybeUninit::new(init)),
        }
    }

    pub fn modify<F: FnOnce(T) -> T>(&self, modify: F) -> &T {
        if !self.modified.load(Ordering::Acquire) {
            let _lock = self.lock.lock().unwrap();
            if !self.modified.load(Ordering::Relaxed) {
                let value = unsafe {
                    mem::replace( &mut*self.value.get(), MaybeUninit::uninit()).assume_init()
                };
                // SAFETY: If `modify` panics, it should poison the lock
                // so it cannot be accessed again, since `self.modified` is false.
                let new_value = modify(value);
                unsafe {
                    *(&mut *self.value.get()) = MaybeUninit::new(new_value);
                }
                self.modified.store(true, Ordering::Release);
            } else {
                // We raced and can fall through
            }
        }

        unsafe { (&*self.value.get()).assume_init_ref() }
    }
}

impl<T> Deref for LazyModify<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        if !self.modified.load(Ordering::Acquire) {
            panic!("must explicitly modify LazyModify before dereferencing")
        }

        unsafe { (&*self.value.get()).assume_init_ref() }
    }
}
