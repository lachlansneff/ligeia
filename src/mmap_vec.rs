// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use mapr::{MmapMut, MmapOptions};
use std::{fs::File, io, iter, marker::PhantomData, mem, slice::SliceIndex};

pub trait VariableWrite<Parent: VariableLength = Self> {
    /// Maximum possible length (in bytes). Should be small.
    fn max_size(meta: <Parent as VariableLength>::Meta) -> usize;
    fn write_variable(self, meta: <Parent as VariableLength>::Meta, b: &mut [u8]) -> usize;
}

pub trait VariableRead<'a, Parent: VariableLength = Self>: Sized {
    fn read_variable(meta: <Parent as VariableLength>::Meta, b: &'a [u8]) -> (Self, usize);
}

// default impl<T: VariableLength> WriteData for T {
//     type Parent = Self;
//     fn write_bytes(&self, _meta: &<Self as VariableLength>::Meta, _b: &mut [u8]) -> usize {
//         unimplemented!()
//     }
// }

/// Variable length data.
pub trait VariableLength {
    type Meta: Copy;
    type DefaultReadData;
}

/// This trait means that Self is variable-sized,
/// but a consistent size while it's useful for the caller.
pub trait ConsistentSize<Parent: VariableLength = Self> {
    fn size(meta: <Parent as VariableLength>::Meta) -> usize;
}

pub enum ReallocEnabled {}
pub enum ReallocDisabled {}

pub trait ReallocOption {
    const ENABLED: bool;
}
impl ReallocOption for ReallocEnabled {
    const ENABLED: bool = true;
}
impl ReallocOption for ReallocDisabled {
    const ENABLED: bool = false;
}

/// Creates an appendable vector that's backed by
/// an anonymous, temporary file, so it can contain more data than
/// fits in physical memory.
pub struct VarMmapVec<T, Realloc: ReallocOption = ReallocEnabled> {
    f: File,
    mapping: MmapMut,
    offset: usize,
    cap: usize,
    _marker: PhantomData<(T, Realloc)>,
}

impl<T: VariableLength, Realloc: ReallocOption> VarMmapVec<T, Realloc> {
    pub fn create() -> io::Result<Self> {
        let f = tempfile::tempfile()?;

        let cap = 4096; // Let's try a single page to start.

        f.set_len(cap)?;

        let mapping = unsafe { MmapOptions::new().len(cap as usize).map_mut(&f)? };

        Ok(Self {
            f,
            mapping,
            offset: 0,
            cap: cap as usize,
            _marker: PhantomData,
        })
    }

    pub fn create_with_capacity(cap: usize) -> io::Result<Self> {
        let f = tempfile::tempfile()?;

        let cap = (cap as u64 + 4096 - 1) & !(4096 - 1);

        f.set_len(cap)?;

        let mapping = unsafe { MmapOptions::new()
            .len(cap as usize)
            .map_mut(&f)? };

        Ok(Self {
            f,
            mapping,
            offset: 0,
            cap: cap as usize,
            _marker: PhantomData,
        })
    }

    /// Returns offset of item in buffer.
    pub fn push<Data>(&mut self, meta: T::Meta, data: Data) -> u64
    where
        Data: VariableWrite<T>,
    {
        if self.offset + Data::max_size(meta) >= self.cap {
            self.realloc();
        }

        let offset = self.offset;

        self.offset += data.write_variable(meta, &mut self.mapping[self.offset..]);
        offset as _
    }

    #[track_caller]
    pub fn window<'a, B>(&mut self, bounds: B) -> VarVecWindow<T>
    where
        B: SliceIndex<[u8], Output = [u8]>,
    {
        VarVecWindow {
            origin: self.mapping.as_ptr(),
            data: &mut self.mapping[bounds],
            _marker: PhantomData,
        }
    }

    // pub fn push_reverse<Data>(&mut self, meta: T::Meta, data: Data) -> u64
    // where
    //     Data: VariableWrite<T> + ConsistentSize<T>
    // {
    //     let size = Data::size(meta);
    //     assert!(self.offset >= size, "cannot push_reverse data that won't fit");

    //     self.offset -= size;
    //     data.write_variable(meta, &mut self.mapping[self.offset..]);
    //     *self.count += 1;
    //     self.offset as _
    // }

    pub fn current_offset(&self) -> u64 {
        self.offset as _
    }

    // pub fn set_offset(&mut self, offset: usize) {
    //     assert!(offset < self.cap);
    //     self.offset = offset;
    // }

    #[cold]
    fn realloc(&mut self) {
        if !Realloc::ENABLED {
            panic!("this instance of `VarMmapVec` is not allowed to reallocate");
        }

        self.cap *= 2;
        self.f
            .set_len(self.cap as u64)
            .expect("failed to extend file");
        let mapping = unsafe {
            MmapOptions::new()
                .len(self.cap)
                .map_mut(&self.f)
                .expect("failed to create to mapping")
        };
        self.mapping = mapping;
    }

    // pub fn iter<Data>(&self) -> VarVecIter<T, Data> {
    //     VarVecIter {
    //         bytes: &self.mapping[16..],
    //         count: *self.count,
    //         _marker: PhantomData,
    //     }
    // }

    // pub fn flush(&self) {
    //     self.mapping.flush().expect("failed to flush mapping");
    // }

    /// This will return garbage if the offset is not aligned to the beginning
    /// of a variable-length item.
    pub fn get_at<'a, Data>(&'a self, meta: T::Meta, offset: u64) -> Data
    where
        Data: VariableRead<'a, T>,
    {
        // T::from_bytes(&mut &self.mapping[offset..])
        Data::read_variable(meta, &self.mapping[offset as usize..]).0
    }

    /// This hints to the operating system that this mapping
    /// is going to be accessed in a random fashion.
    pub fn hint_random_access(&self) {
        use madvise::{AccessPattern, AdviseMemory as _};

        // Ignore errors.
        let _ = self.mapping.advise_memory_access(AccessPattern::Random);
    }
}

pub struct VarVecWindow<'a, T> {
    origin: *const u8,
    data: &'a mut [u8],
    _marker: PhantomData<T>,
}

impl<'a, T: VariableLength> VarVecWindow<'a, T> {
    #[track_caller]
    pub fn take_window(&mut self, size: usize) -> VarVecWindow<'a, T> {
        let data = mem::take(&mut self.data);
        let (first, second) = data.split_at_mut(size);
        self.data = second;

        Self {
            origin: self.origin,
            data: first,
            _marker: PhantomData,
        }
    }

    pub fn offset_in_mapping(&self) -> usize {
        unsafe { self.data.as_ptr().offset_from(self.origin) as usize }
    }

    pub fn iter<Data>(&'a self, meta: T::Meta) -> impl Iterator<Item=Data> + 'a
    where
        Data: VariableRead<'a, T>,
        T::Meta: Copy,
    {
        let mut data = &self.data[..];
        iter::from_fn(move || {
            if data.len() == 0 {
                None
            } else {
                let (value, size) = Data::read_variable(meta, data);
                data = &data[size..];
                Some(value)
            }
        })
    }
}

impl<'a, T: VariableLength> VarVecWindow<'a, T> {
    pub fn push<Data>(&mut self, meta: T::Meta, data: Data)
    where
        Data: VariableWrite<T>
    {
        assert!(self.data.len() > 0, "the window has zero size");
        let slice = mem::take(&mut self.data);
        let size = data.write_variable(meta, slice);
        self.data = &mut slice[size..];
    }
}

impl<'a, T: VariableLength> VarVecWindow<'a, T> {
    pub fn push_rev<Data>(&mut self, meta: T::Meta, data: Data)
    where
        Data: VariableWrite<T> + ConsistentSize<T>
    {
        assert!(self.data.len() > 0, "the window has zero size");
        let size = Data::size(meta);
        let slice = mem::take(&mut self.data);
        let (rest, dest) = slice.split_at_mut(slice.len() - size);
        self.data = rest;

        data.write_variable(meta, dest);        
    }
}

// pub struct VarVecIter<'a, T: VariableLength, Data = <T as VariableLength>::DefaultReadData> {
//     bytes: &'a [u8],
//     count: u64,
//     _marker: PhantomData<(T, Data)>,
// }

// impl<'a, T: VariableLength, Data: ReadData<'a, T>> VarVecIter<'a, T, Data> {
//     pub fn next_data(&mut self, meta: T::Meta) -> Option<Data> {
//         if self.count > 0 {
//             self.count -= 1;
//             let (data, offset) = Data::read_data(meta, &mut self.bytes);
//             self.bytes = &self.bytes[offset..];
//             Some(data)
//         } else {
//             None
//         }
//     }
// }

// impl<'a, T: VariableLength> Iterator for MmapVecIter<'a, T> {
//     type Item = T;
//     fn next(&mut self) -> Option<Self::Item> {
//         if self.count > 0 {
//             self.count -= 1;
//             Some(T::from_bytes(&mut self.data))
//         } else {
//             None
//         }
//     }
// }

// pub unsafe trait Pod: Copy + Sized {}

// unsafe impl Pod for u64 {}

// struct ConstantLength<T>(T);

// impl<T: Pod> WriteData for ConstantLength<T> {
//     fn max_size(_: ()) -> usize {
//         mem::size_of::<T>()
//     }

//     fn write_bytes(self, _: (), b: &mut [u8]) -> usize {
//         assert!(b.len() >= mem::size_of::<T>());
//         unsafe { *(b.as_mut_ptr() as *mut T) = self.0 };
//         mem::size_of::<T>()
//     }
// }

// impl<'a, T: Pod> ReadData<'a> for ConstantLength<T> {
//     fn read_data(_: (), b: &[u8]) -> (Self, usize) {
//         assert!(b.len() >= mem::size_of::<T>());
//         let v = unsafe { *(b.as_ptr() as *const T) };

//         (Self(v), mem::size_of::<T>())
//     }
// }

// impl<T: Pod> VariableLength for ConstantLength<T> {
//     type Meta = ();
//     type DefaultReadData = Self;
// }

// pub struct MmapVec<T> {
//     inner: VarMmapVec<ConstantLength<T>>,
//     len: usize,
// }

// impl<T: Pod> MmapVec<T> {
//     pub unsafe fn create() -> io::Result<Self> {
//         Ok(Self {
//             inner: VarMmapVec::create()?,
//             len: 0,
//         })
//     }

//     pub unsafe fn create_with_capacity(capacity: usize) -> io::Result<Self> {
//         Ok(Self {
//             inner: VarMmapVec::create_with_capacity(capacity * mem::size_of::<T>())?,
//             len: 0,
//         })
//     }

//     /// Returns index of value.
//     pub fn push(&mut self, v: T) -> usize {
//         self.inner.push((), ConstantLength(v));
//         let index = self.len;
//         self.len += 1;
//         index
//     }
// }

// impl<T: Pod> Deref for MmapVec<T> {
//     type Target = [T];

//     fn deref(&self) -> &Self::Target {
//         let slice = &self.inner.mapping[16..];
//         unsafe {
//             slice::from_raw_parts(slice.as_ptr() as *const T, self.len)
//         }
//     }
// }

// impl<T: Pod> DerefMut for MmapVec<T> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         let slice = &mut self.inner.mapping[16..];
//         unsafe {
//             slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut T, self.len)
//         }
//     }
// }
