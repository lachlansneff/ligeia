use std::{collections::{BTreeMap, HashMap}, convert::{TryFrom, TryInto}, fmt::{Debug, Display, Formatter}, io::{self, Read}, mem, num::NonZeroU64, slice, sync::{Arc, Mutex}, time::Instant};
use vcd::{Command, Parser, ScopeItem, Value};

use crate::{
    mmap_vec::{MmapVec, Pod, ReadData, VarMmapVec, VariableLength, WriteData},
    types::{Qit, QitSlice},
};

pub struct NotValidVarIdError(());

impl Debug for NotValidVarIdError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to convert to VarId")
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VarId(NonZeroU64);

impl VarId {
    pub fn new(x: u64) -> Result<Self, NotValidVarIdError> {
        if x == u64::max_value() {
            Err(NotValidVarIdError(()))
        } else {
            Ok(Self(unsafe {
                NonZeroU64::new_unchecked(x + 1)
            }))
        }
    }

    pub fn get(&self) -> u64 {
        self.0.get() - 1
    }
}

impl TryFrom<u64> for VarId {
    type Error = NotValidVarIdError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<VarId> for u64 {
    fn from(var_id: VarId) -> Self {
        var_id.get()
    }
}

impl Debug for VarId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <u64 as Debug>::fmt(&self.get(), f)
    }
}

impl Display for VarId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <u64 as Display>::fmt(&self.get(), f)
    }
}

// pub type VarId = u64;

#[derive(Debug)]
pub struct VarInfo {
    pub name: String,
    pub qits: u32,
}

#[derive(Debug)]
pub struct Scope {
    pub name: String,
    pub scopes: Vec<Scope>,
    pub vars: Vec<VarId>,
}

pub struct VarTree {
    pub scope: Scope,
    pub variables: BTreeMap<VarId, VarInfo>,
}

fn find_all_scopes_and_variables(header: vcd::Header) -> VarTree {
    fn recurse(variables: &mut BTreeMap<VarId, VarInfo>, items: impl Iterator<Item=vcd::ScopeItem>) -> (Vec<Scope>, Vec<VarId>) {
        let mut scopes = vec![];
        let mut vars = vec![];

        for item in items {
            match item {
                ScopeItem::Var(var) => {
                    let id = var.code.number().try_into().unwrap();
                    vars.push(id);
                    variables.insert(id, VarInfo {
                        name: var.reference,
                        qits: var.size,
                    });
                }
                ScopeItem::Scope(scope) => {
                    let (sub_scopes, sub_vars) = recurse(variables, scope.children.into_iter());
                    scopes.push(Scope {
                        name: scope.identifier,
                        scopes: sub_scopes,
                        vars: sub_vars,
                    });
                }
            }
        }

        (scopes, vars)
    }

    // let (name, top_items) = header.items.into_iter().find_map(|item| {
    //     if let ScopeItem::Scope(scope) = item {
    //         Some((scope.identifier, scope.children))
    //     } else {
    //         None
    //     }
    // }).expect("failed to find top-level scope in vcd file");

    let mut variables = BTreeMap::new();
    let (scopes, vars) = recurse(&mut variables, header.items.into_iter());

    // INFO: Turns out the variable ids are usually sequential, but not always
    // let mut previous = vars[0].id;
    // for var in vars[1..].iter() {
    //     if var.id != previous + 1 {
    //         eprintln!("wasn't sequential at {}", var.id);
    //     }
    //     previous = var.id;
    // }

    VarTree {
        scope: Scope {
            name: "top".to_string(),
            scopes,
            vars,
        },
        variables,
    }
}

impl WriteData for u64 {
    #[inline]
    fn max_size(_: ()) -> usize {
        10
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

impl WriteData for VarId {
    #[inline]
    fn max_size(_: ()) -> usize {
        10
    }

    fn write_bytes(self, _: (), b: &mut [u8]) -> usize {
        // leb128::write::unsigned(&mut b, self.0.get()).unwrap()
        varint_simd::encode_to_slice(self.0.get(), b) as usize
    }
}
impl ReadData<'_> for VarId {
    fn read_data(_: (), b: &[u8]) -> (Self, usize) {
        let (int, size) = varint_simd::decode(b).unwrap();
        (VarId(unsafe { NonZeroU64::new_unchecked(int) }), size as _)
    }
}

impl VariableLength for VarId {
    type Meta = ();
    type DefaultReadData = Self;
}

impl From<Value> for Qit {
    fn from(v: Value) -> Self {
        match v {
            Value::X => Qit::X,
            Value::Z => Qit::Z,
            Value::V0 => Qit::Zero,
            Value::V1 => Qit::One,
        }
    }
}

pub trait WriteQits: IntoIterator<Item = Qit> {
    fn write_qits(self, bytes: &mut [u8]);
}

impl<T: IntoIterator<Item = Qit>> WriteQits for T {
    #[inline]
    fn write_qits(self, bytes: &mut [u8]) {
        let mut iter = self.into_iter();
        for byte in bytes.iter_mut() {
            for (i, qit) in (&mut iter).take(4).enumerate() {
                let raw_qit = match qit {
                    Qit::X => 0b00,
                    Qit::Z => 0b01,
                    Qit::Zero => 0b10,
                    Qit::One => 0b11,
                };

                *byte |= raw_qit << (i * 2);
            }
        }
    }
}

enum StreamingValueChange {}

#[derive(Debug)]
pub struct StreamingVCBits<T: WriteQits> {
    pub var_id: VarId,
    pub offset_to_prev: u64,
    pub offset_to_prev_timestamp: u64,
    pub qits: T,
}

impl<T: WriteQits> WriteData<StreamingValueChange> for StreamingVCBits<T> {
    #[inline]
    fn max_size(qits: usize) -> usize {
        <u64 as WriteData>::max_size(()) * 3 + Qit::bits_to_bytes(qits)
    }

    fn write_bytes(self, qits: usize, mut b: &mut [u8]) -> usize {
        let mut header = self.var_id.write_bytes((), &mut b);
        header += self.offset_to_prev.write_bytes((), &mut b[header..]);
        header += self.offset_to_prev_timestamp.write_bytes((), &mut b[header..]);

        let bytes = Qit::bits_to_bytes(qits);

        self.qits.write_qits(&mut b[header..header + bytes]);

        header + bytes
    }
}
impl<'a> ReadData<'a, StreamingValueChange> for StreamingVCBits<QitSlice<'a>> {
    fn read_data(qits: usize, b: &'a [u8]) -> (Self, usize) {
        let (var_id, mut offset) = VarId::read_data((), b);
        let (offset_to_prev, size) = u64::read_data((), &b[offset..]);
        offset += size;
        let (offset_to_prev_timestamp, size) = u64::read_data((), &b[offset..]);
        offset += size;

        let bytes = Qit::bits_to_bytes(qits);

        let data = StreamingVCBits {
            var_id,
            offset_to_prev,
            offset_to_prev_timestamp,
            qits: QitSlice::new(qits, &b[offset..offset + bytes]),
        };

        (data, offset + bytes)
    }
}

impl VariableLength for StreamingValueChange {
    type Meta = usize;
    type DefaultReadData = ();
}

/// The variable id is the index of this in the `var_data` structure.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct StreamingVarMeta {
    var_id: VarId,
    qits: u32,
    last_value_change_offset: u64,
    number_of_value_changes: u64,
    last_timestamp_offset: u64,
    last_timestamp: u64,
}

/// Hopefully this isn't a terrible idea.
unsafe impl Pod for Option<StreamingVarMeta> {}

/// Used to efficiently convert from a vcd that's larger than memory
/// to a structure that can be easily traversed in order to create a
/// db that can be easily and quickly queried.
pub struct StreamingDb {
    /// All var ids + some metadata, padded for each var_id to correspond exactly to its index in this array.
    var_metas: MmapVec<Option<StreamingVarMeta>>,

    /// A list of timestamps, stored as the delta since the previous timestamp.
    timestamp_chain: VarMmapVec<u64>,

    value_change: VarMmapVec<StreamingValueChange>,

    var_tree: VarTree,
}

impl StreamingDb {
    pub fn load_vcd<R: Read>(parser: &mut Parser<R>) -> io::Result<Self> {
        let header = parser.parse_header()?;

        let var_tree = find_all_scopes_and_variables(header);

        let mut var_metas = unsafe { MmapVec::create_with_capacity(var_tree.variables.len())? };

        let mut previous = 0;
        for (&var_id, var_info) in &var_tree.variables {
            while previous + 1 < var_id.get() {
                // Not contiguous, lets pad until it is.
                var_metas.push(None);
                previous += 1;
            }
            previous = var_id.get();

            let index = var_metas.push(Some(StreamingVarMeta {
                var_id,
                qits: var_info.qits,
                last_value_change_offset: 0,
                number_of_value_changes: 0,
                last_timestamp_offset: 0,
                last_timestamp: 0,
            }));
            assert_eq!(index, var_id.get() as _, "var_meta index is not equal to the var_id");
        }

        let mut timestamp_chain = unsafe { VarMmapVec::create()? };
        let mut value_change = unsafe { VarMmapVec::create()? };

        let mut processed_commands_count = 0;
        let start = Instant::now();

        let mut last_timestamp = 0;
        let mut timestamp_offset = 0;
        let mut number_of_timestamps = 0;

        while let Some(command) = parser.next_command() {
            let command = command?;
            match command {
                Command::Timestamp(timestamp) => {
                    timestamp_offset = timestamp_chain.push((), timestamp - last_timestamp);
                    last_timestamp = timestamp;

                    processed_commands_count += 1;
                    number_of_timestamps += 1;
                }
                Command::ChangeVector(code, values) => {
                    let var_id: VarId = code.number().try_into().unwrap();
                    let var_meta = var_metas[var_id.get() as usize].as_mut().unwrap();

                    var_meta.last_value_change_offset = value_change.push(values.len(), StreamingVCBits {
                        var_id,
                        offset_to_prev: value_change.current_offset() - var_meta.last_value_change_offset,
                        offset_to_prev_timestamp: timestamp_offset - var_meta.last_timestamp_offset,
                        qits: values.into_iter().copied().map(Into::into)
                    });
                    var_meta.number_of_value_changes += 1;
                    var_meta.last_timestamp_offset = timestamp_offset;
                    var_meta.last_timestamp = last_timestamp;

                    processed_commands_count += 1;
                }
                Command::ChangeScalar(code, value) => {
                    let var_id: VarId = code.number().try_into().unwrap();
                    let var_meta = var_metas[var_id.get() as usize].as_mut().unwrap();

                    var_meta.last_value_change_offset = value_change.push(1, StreamingVCBits {
                        var_id,
                        offset_to_prev: value_change.current_offset() - var_meta.last_value_change_offset,
                        offset_to_prev_timestamp: timestamp_offset - var_meta.last_timestamp_offset,
                        qits: Some(value.into()).into_iter(),
                    });
                    var_meta.number_of_value_changes += 1;
                    var_meta.last_timestamp_offset = timestamp_offset;
                    var_meta.last_timestamp = last_timestamp;

                    processed_commands_count += 1;
                }
                _ => {},
            }
        }

        let elapsed = start.elapsed();
        println!("processed {} commands in {:.2} seconds", processed_commands_count, elapsed.as_secs_f32());
        println!("contained {} timestamps", number_of_timestamps);
        println!("last timestamp: {}", last_timestamp);

        Ok(Self {
            var_metas,
            timestamp_chain,
            value_change,
            var_tree,
        })
    }

    pub fn var_tree(&self) -> &VarTree {
        &self.var_tree
    }

    /// Iterates backward through the value changes for a specific variable.
    pub fn iter_reverse_value_change(&self, var_id: VarId) -> ReverseValueChangeIter {
        let var_meta = &self.var_metas[var_id.get() as usize].unwrap();

        ReverseValueChangeIter {
            value_changes: &self.value_change,
            timestamp_chain: &self.timestamp_chain,
            current_timestamp: var_meta .last_timestamp,
            current_timestamp_offset: var_meta.last_timestamp_offset,
            current_value_change_offset: var_meta.last_value_change_offset,
            remaining: var_meta.number_of_value_changes,
            qits: var_meta.qits as usize,
        }
    }
}

/// Iterates backward through the value changes for a specific variable.
pub struct ReverseValueChangeIter<'a> {
    value_changes: &'a VarMmapVec<StreamingValueChange>,
    timestamp_chain: &'a VarMmapVec<u64>,
    current_timestamp: u64,
    current_timestamp_offset: u64,
    current_value_change_offset: u64,
    remaining: u64,
    qits: usize,
}

impl<'a> Iterator for ReverseValueChangeIter<'a> {
    /// (timestamp, qits)
    type Item = (u64, QitSlice<'a>);

    fn next(&mut self) -> Option<(u64, QitSlice<'a>)> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;

        let timestamp_delta: u64 = self.timestamp_chain.get_at((), self.current_timestamp_offset);
        let timestamp = self.current_timestamp;
        self.current_timestamp -= timestamp_delta;

        let value_change: StreamingVCBits<QitSlice<'a>> = self.value_changes.get_at(self.qits, self.current_value_change_offset);

        self.current_value_change_offset -= value_change.offset_to_prev;
        self.current_timestamp_offset -= value_change.offset_to_prev_timestamp;

        Some((timestamp, value_change.qits))
    }
}

impl ExactSizeIterator for ReverseValueChangeIter<'_> {
    fn len(&self) -> usize {
        self.remaining as _
    }
}

const NODE_CHILDREN: usize = 8;

/// While this is variable size (through the `VariableLength` trait),
/// each node in a given layer for a given variable is the same size.
#[derive(Debug)]
struct Node<T: WriteQits> {
    /// Offsets (from offset of beginning of the tree this node is in) of this node's children.
    children: [u32; NODE_CHILDREN],
    averaged_qits: T,
}

enum NodeProxy {}

impl<T: WriteQits> WriteData<NodeProxy> for Node<T> {
    fn max_size(qits: usize) -> usize {
        mem::size_of::<[u32; NODE_CHILDREN]>() + Qit::bits_to_bytes(qits)
    }

    fn write_bytes(self, qits: usize, b: &mut [u8]) -> usize {
        let total_size = Self::max_size(qits);
        let children_size = mem::size_of::<[u32; NODE_CHILDREN]>();
        
        b[..children_size]
            .copy_from_slice(unsafe {
                slice::from_raw_parts(self.children.as_ptr() as *const u8, self.children.len() * mem::size_of::<u32>())
            });

        self.averaged_qits.write_qits(&mut b[children_size..total_size]);
        
        total_size
    }
}
impl<'a> ReadData<'a, NodeProxy> for Node<QitSlice<'a>> {
    fn read_data(qits: usize, b: &'a [u8]) -> (Self, usize) {
        let total_size = Self::max_size(qits);
        let children_size = mem::size_of::<[u32; NODE_CHILDREN]>();

        let children = unsafe { *(b[children_size..].as_ptr() as *const [u32; NODE_CHILDREN]) };

        let node = Node {
            children,
            averaged_qits: QitSlice::new(qits, &b[children_size..total_size])
        };

        (node, total_size)
    }
}

// struct JustBits<T: WriteBits>(T);
impl<T: WriteQits> WriteData<NodeProxy> for T {
    fn max_size(qits: usize) -> usize {
        // (qits as usize + 4 - 1) / 4
        Qit::bits_to_bytes(qits)
    }

    fn write_bytes(self, qits: usize, b: &mut [u8]) -> usize {
        let bytes = (qits as usize + 8 - 1) / 8;

        self.write_qits(&mut b[..bytes]);

        bytes
    }
}
impl<'a> ReadData<'a, NodeProxy> for QitSlice<'a> {
    fn read_data(qits: usize, b: &'a [u8]) -> (Self, usize) {
        let bytes = Self::max_size(qits);
        (QitSlice::new(qits, &b[..bytes]), bytes)
    }
}

impl VariableLength for NodeProxy {
    type Meta = usize;
    type DefaultReadData = ();
}

/// Converts the number of value changes to the number of layers in the tree.
fn value_changes_to_layers(count: usize) -> usize {
    todo!("value change count: {}", count)
}

/// This contains a list of variably-sized structures that are structurally similar
/// to a tree, each one corresponding to variable.
/// Each layer is made up of nodes and each tree looks like the following:
/// | up-to 128 top-level nodes |
///       / / /   |   \ \ \
///              ...
/// `value_changes_to_layers() - 2` layers
///              ...
/// | a final layer of nodes corresponding to the actual values (only bit iterators are written in this layer) |
///
///
/// This structure utilizes multiple threads and prioritises variables that are requested.
struct FrustumTree {
    /// Contains a map of variable ids to offsets in the tree structure.
    offsets: HashMap<VarId, u64>,
    trees: VarMmapVec<NodeProxy>,

    queue: Arc<Mutex<Vec<VarId>>>,
    // thread_pool: Option<rayon::ThreadPool>,
}

impl FrustumTree {
    pub fn generate(streaming_db: StreamingDb) -> Self {
        // let thread_pool = rayon::ThreadPoolBuilder::new().build().expect("failed to build threadpool");
        // let num_threads = thread_pool.current_num_threads();

        

        todo!()
    }

    pub fn is_finished(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
    }
}

pub struct QueryDb {
    
    // frustum_tree: 
}
