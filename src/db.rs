use std::{collections::{BTreeMap, HashMap}, convert::{TryFrom, TryInto}, fmt::{Debug, Display, Formatter}, io::{self, Read}, mem, num::NonZeroU64, path::Path, slice, sync::{Arc, Mutex}, time::Instant};
use svcb::{StorageDeclaration, Timestep, VariableDeclaration};

use crate::{mmap_vec::{ReadData, VarMmapVec, VariableLength, WriteData}, svcb, types::{Qit, QitSlice}};

impl WriteData for u64 {
    #[inline]
    fn max_size(_: ()) -> usize {
        <u64 as varint_simd::num::VarIntTarget>::MAX_VARINT_BYTES as _
    }

    fn write_bytes(self, _: (), b: &mut [u8]) -> usize {
        // leb128::write::unsigned(&mut b, *self).unwrap()
        varint_simd::encode_to_slice(self, b) as usize
    }
}
impl ReadData<'_> for u64 {
    fn read_data(_: (), b: &[u8]) -> (Self, usize) {
        varint_simd::decode(b)
            .map(|(int, size)| (int, size as _))
            .unwrap()
    }
}

impl VariableLength for u64 {
    type Meta = ();
    type DefaultReadData = Self;
}

impl WriteData for u32 {
    #[inline]
    fn max_size(_: ()) -> usize {
        <u32 as varint_simd::num::VarIntTarget>::MAX_VARINT_BYTES as _
    }

    fn write_bytes(self, _: (), b: &mut [u8]) -> usize {
        // leb128::write::unsigned(&mut b, *self).unwrap()
        varint_simd::encode_to_slice(self, b) as usize
    }
}
impl ReadData<'_> for u32 {
    fn read_data(_: (), b: &[u8]) -> (Self, usize) {
        varint_simd::decode(b)
            .map(|(int, size)| (int, size as _))
            .unwrap()
    }
}

impl VariableLength for u32 {
    type Meta = ();
    type DefaultReadData = Self;
}

pub trait WaveformDatabase: Send {

}

pub trait WaveformLoader: Sync {
    fn supports_file_extension(&self, s: &str) -> bool;
    fn description(&self) -> String;
    
    /// A file is technically a stream, but generally, specializing parsers for files can be more efficient than parsing
    /// from a generic reader.
    fn load_file(&self, path: &Path) -> anyhow::Result<Box<dyn WaveformDatabase>>;
    fn load_stream(&self, reader: Box<dyn Read>) -> anyhow::Result<Box<dyn WaveformDatabase>>;
}


// const NODE_CHILDREN: usize = 8;

// /// While this is variable size (through the `VariableLength` trait),
// /// each node in a given layer for a given variable is the same size.
// #[derive(Debug)]
// struct Node<T: WriteQits> {
//     /// Offsets (from offset of beginning of the tree this node is in) of this node's children.
//     children: [u32; NODE_CHILDREN],
//     averaged_qits: T,
// }

// enum NodeProxy {}

// impl<T: WriteQits> WriteData<NodeProxy> for Node<T> {
//     fn max_size(qits: usize) -> usize {
//         mem::size_of::<[u32; NODE_CHILDREN]>() + Qit::bits_to_bytes(qits)
//     }

//     fn write_bytes(self, qits: usize, b: &mut [u8]) -> usize {
//         let total_size = Self::max_size(qits);
//         let children_size = mem::size_of::<[u32; NODE_CHILDREN]>();
        
//         b[..children_size]
//             .copy_from_slice(unsafe {
//                 slice::from_raw_parts(self.children.as_ptr() as *const u8, self.children.len() * mem::size_of::<u32>())
//             });

//         self.averaged_qits.write_qits(&mut b[children_size..total_size]);
        
//         total_size
//     }
// }
// impl<'a> ReadData<'a, NodeProxy> for Node<QitSlice<'a>> {
//     fn read_data(qits: usize, b: &'a [u8]) -> (Self, usize) {
//         let total_size = Self::max_size(qits);
//         let children_size = mem::size_of::<[u32; NODE_CHILDREN]>();

//         let children = unsafe { *(b[children_size..].as_ptr() as *const [u32; NODE_CHILDREN]) };

//         let node = Node {
//             children,
//             averaged_qits: QitSlice::new(qits, &b[children_size..total_size])
//         };

//         (node, total_size)
//     }
// }

// // struct JustBits<T: WriteBits>(T);
// impl<T: WriteQits> WriteData<NodeProxy> for T {
//     fn max_size(qits: usize) -> usize {
//         // (qits as usize + 4 - 1) / 4
//         Qit::bits_to_bytes(qits)
//     }

//     fn write_bytes(self, qits: usize, b: &mut [u8]) -> usize {
//         let bytes = (qits as usize + 8 - 1) / 8;

//         self.write_qits(&mut b[..bytes]);

//         bytes
//     }
// }
// impl<'a> ReadData<'a, NodeProxy> for QitSlice<'a> {
//     fn read_data(qits: usize, b: &'a [u8]) -> (Self, usize) {
//         let bytes = Self::max_size(qits);
//         (QitSlice::new(qits, &b[..bytes]), bytes)
//     }
// }

// impl VariableLength for NodeProxy {
//     type Meta = usize;
//     type DefaultReadData = ();
// }

// /// Converts the number of value changes to the number of layers in the tree.
// fn value_changes_to_layers(count: usize) -> usize {
//     todo!("value change count: {}", count)
// }

// /// This contains a list of variably-sized structures that are structurally similar
// /// to a tree, each one corresponding to variable.
// /// Each layer is made up of nodes and each tree looks like the following:
// /// | up-to 128 top-level nodes |
// ///       / / /   |   \ \ \
// ///              ...
// /// `value_changes_to_layers() - 2` layers
// ///              ...
// /// | a final layer of nodes corresponding to the actual values (only bit iterators are written in this layer) |
// ///
// ///
// /// This structure utilizes multiple threads and prioritises variables that are requested.
// struct FrustumTree {
//     /// Contains a map of variable ids to offsets in the tree structure.
//     offsets: HashMap<VarId, u64>,
//     trees: VarMmapVec<NodeProxy>,

//     queue: Arc<Mutex<Vec<VarId>>>,
//     // thread_pool: Option<rayon::ThreadPool>,
// }

// impl FrustumTree {
//     pub fn generate(streaming_db: StreamingDb) -> Self {
//         // let thread_pool = rayon::ThreadPoolBuilder::new().build().expect("failed to build threadpool");
//         // let num_threads = thread_pool.current_num_threads();

        

//         todo!()
//     }

//     pub fn is_finished(&self) -> bool {
//         self.queue.lock().unwrap().is_empty()
//     }
// }

// pub struct QueryDb {
    
//     // frustum_tree: 
// }
