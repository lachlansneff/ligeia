use mapr::{MmapMut, MmapOptions};
use std::{fs::File, io, marker::PhantomData};

pub trait WriteData<Parent: VariableLength = Self> {
    /// Maximum possible length (in bytes). Should be small.
    fn max_size(meta: <Parent as VariableLength>::Meta) -> usize;
    fn write_bytes(self, meta: <Parent as VariableLength>::Meta, b: &mut [u8]) -> usize;
}

pub trait ReadData<'a, Parent: VariableLength = Self>: Sized {
    fn read_data(meta: <Parent as VariableLength>::Meta, b: &'a [u8]) -> (Self, usize);
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

    // fn max_size(meta: Self::Meta) -> usize;

    // /// Returns number of bytes written.
    // #[inline]
    // fn write_bytes(mut data: impl WriteData<Self>, meta: Self::Meta, b: &mut [u8]) -> usize
    //     where Self: Sized
    // {
    //     data.write_bytes(meta, b)
    // }

    // /// Reads from `b` as necessary.
    // fn from_bytes<'a, Data: ReadData<'a, Self>>(meta: Self::Meta, b: &'a [u8]) -> (Data, usize)
    //     where Self: Sized
    // {
    //     Data::read_data(meta, b)
    // }
}

/// Creates an appendable vector that's backed by
/// an anonymous, temporary file, so it can contain more data than
/// fits in physical memory.
pub struct VarMmapVec<T> {
    f: File,
    mapping: MmapMut,
    count: &'static mut u64,
    bytes: usize,
    cap: usize,
    _marker: PhantomData<T>,
}

impl<T: VariableLength> VarMmapVec<T> {
    pub unsafe fn create() -> io::Result<Self> {
        let f = tempfile::tempfile()?;

        let cap = 4096; // Let's try a single page to start.

        f.set_len(cap)?;

        let mapping = MmapOptions::new().len(cap as usize).map_mut(&f)?;

        let count = &mut *(mapping.as_ptr() as *mut u64);

        Ok(Self {
            f,
            mapping,
            count,
            bytes: 16,
            cap: cap as usize,
            _marker: PhantomData,
        })
    }

    // unsafe fn create_with_capacity(cap: usize) -> io::Result<Self> {
    //     let f = tempfile::tempfile()?;

    //     let cap = (cap as u64 + 4096 - 1) & !(4096 - 1);

    //     f.set_len(cap)?;

    //     let mapping = MmapOptions::new()
    //         .len(cap as usize)
    //         .map_mut(&f)?;

    //     let count = &mut *(mapping.as_ptr() as *mut u64);

    //     Ok(Self {
    //         f,
    //         mapping,
    //         count,
    //         bytes: 16,
    //         cap: cap as usize,
    //         _marker: PhantomData,
    //     })
    // }

    /// Returns offset of item in buffer.
    pub fn push<Data>(&mut self, meta: T::Meta, data: Data) -> u64
    where
        Data: WriteData<T>,
    {
        if self.bytes + Data::max_size(meta) >= self.cap {
            self.realloc();
        }

        let offset = self.bytes;

        self.bytes += data.write_bytes(meta, &mut self.mapping[self.bytes..]);
        *self.count += 1;
        offset as _
    }

    pub fn current_offset(&self) -> u64 {
        self.bytes as _
    }

    #[cold]
    fn realloc(&mut self) {
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
        self.count = unsafe { &mut *(self.mapping.as_ptr() as *mut u64) };
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
        Data: ReadData<'a, T>,
    {
        // T::from_bytes(&mut &self.mapping[offset..])
        Data::read_data(meta, &self.mapping[offset as usize..]).0
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
