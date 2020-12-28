use std::{collections::HashMap, convert::{TryFrom, TryInto}, fmt::{Debug, Display, Formatter}, io::{self, Read}, mem, num::NonZeroU64, slice};
use vcd::{Command, Parser, ScopeItem, Value};

use crate::mmap_vec::{MmapVec, Pod, VarMmapVec, VariableLength, WriteData, ReadData};

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
        if (1u64 << 63) & x != 0 {
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
    name: String,
    bits: u32,
}

#[derive(Debug)]
pub struct Scope {
    name: String,
    scopes: Vec<Scope>,
    vars: Vec<VarId>,
}

pub struct VarTree {
    scope: Scope,
    variables: HashMap<VarId, VarInfo>,
}

fn find_all_scopes_and_variables(header: vcd::Header) -> VarTree {
    fn recurse(variables: &mut HashMap<VarId, VarInfo>, items: impl Iterator<Item=vcd::ScopeItem>) -> (Vec<Scope>, Vec<VarId>) {
        let mut scopes = vec![];
        let mut vars = vec![];

        for item in items {
            match item {
                ScopeItem::Var(var) => {
                    let id = var.code.number().try_into().unwrap();
                    vars.push(id);
                    variables.insert(id, VarInfo {
                        name: var.reference,
                        bits: var.size,
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

    let (name, top_items) = header.items.into_iter().find_map(|item| {
        if let ScopeItem::Scope(scope) = item {
            Some((scope.identifier, scope.children))
        } else {
            None
        }
    }).expect("failed to find top-level scope in vcd file");

    let mut variables = HashMap::new();
    let (scopes, vars) = recurse(&mut variables, top_items.into_iter());

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
            name, scopes, vars,
        },
        variables,
    }
}

impl WriteData for u64 {
    #[inline]
    fn max_size(_: ()) -> usize {
        10
    }

    fn write_bytes(&mut self, _: (), b: &mut [u8]) -> usize {
        // leb128::write::unsigned(&mut b, *self).unwrap()
        varint_simd::encode_to_slice(*self, b) as usize
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

    fn write_bytes(&mut self, _: (), b: &mut [u8]) -> usize {
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bit {
    X,
    Z,
    Zero,
    One,
}

impl From<Value> for Bit {
    fn from(v: Value) -> Self {
        match v {
            Value::X => Bit::X,
            Value::Z => Bit::Z,
            Value::V0 => Bit::Zero,
            Value::V1 => Bit::One,
        }
    }
}

#[derive(Copy, Clone)]
pub struct BitIter<'a> {
    bits: usize,
    index: usize,
    data: &'a [u8],
}

impl<'a> BitIter<'a> {
    pub fn new(bits: usize, data: &'a [u8]) -> Self {
        Self {
            bits,
            index: 0,
            data,
        }
    }

    pub fn num_bits(&self) -> usize {
        self.bits
    }
}

impl<'a> Iterator for BitIter<'a> {
    type Item = Bit;
    fn next(&mut self) -> Option<Bit> {
        if self.index < self.bits {
            let byte = self.data[self.index / 8];
            let in_index = (self.index * 2) % 8;
            let bit = (byte & (0b11 << in_index)) >> in_index;
            self.index += 1;

            Some(match bit {
                0b00 => Bit::X,
                0b01 => Bit::Z,
                0b10 => Bit::Zero,
                0b11 => Bit::One,
                _ => unreachable!(),
            })
        } else {
            None
        }
    }
}

impl Display for BitIter<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // let mut remaining = self.bits;
        // for chunk in self.data.chunks(mem::size_of::<u128>()) {
        //     let width = remaining % 128;
        //     let mut bits: u128 = 0;
        //     for (i, byte) in chunk.iter().enumerate() {
        //         if remaining < 8 {
        //             for j in 0..remaining {
        //                 let bit = (*byte & (1 << j) != 0) as u128;
        //                 bits |= bit << ((i * 8) + j);
        //             }
        //         } else {
        //             bits |= (*byte as u128) << (i * 8);
        //             remaining -= 8;
        //         }
        //     }
        //     write!(f, "{:0width$b}", bits, width = width)?;
        // }

        for bit in *self {
            match bit {
                Bit::X => write!(f, "x")?,
                Bit::Z => write!(f, "z")?,
                Bit::Zero => write!(f, "0")?,
                Bit::One => write!(f, "1")?,
            }
        }

        Ok(())
    }
}

impl Debug for BitIter<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

pub trait WriteBits: Iterator<Item = Bit> {
    fn write_bits(&mut self, bytes: &mut [u8]);
}

impl<T: Iterator<Item = Bit>> WriteBits for T {
    #[inline]
    fn write_bits(&mut self, bytes: &mut [u8]) {
        for byte in bytes.iter_mut() {
            for (i, bit) in self.take(4).enumerate() {
                let raw_bit = match bit {
                    Bit::X => 0b00,
                    Bit::Z => 0b01,
                    Bit::Zero => 0b10,
                    Bit::One => 0b11,
                };

                *byte |= raw_bit << (i * 2);
            }
        }
    }
}

enum StreamingValueChange {}

#[derive(Debug)]
pub struct StreamingVCBits<T: WriteBits> {
    pub var_id: VarId,
    pub offset_to_prev: u64,
    pub timestamp_delta_index: u64,
    pub bits: T,
}

impl<T: WriteBits> WriteData<StreamingValueChange> for StreamingVCBits<T> {
    #[inline]
    fn max_size(bits: usize) -> usize {
        <u64 as WriteData>::max_size(()) * 3 + ((bits + 8 - 1) / 8)
    }

    fn write_bytes(&mut self, bits: usize, mut b: &mut [u8]) -> usize {
        let mut header = self.var_id.write_bytes((), &mut b);
        header += self.offset_to_prev.write_bytes((), &mut b[header..]);
        header += self.timestamp_delta_index.write_bytes((), &mut b[header..]);

        let bytes = (bits + 8 - 1) / 8;

        // for byte in b[header..header + bytes].iter_mut() {
        //     for (i, bit) in (&mut self.bits).take(4).enumerate() {
        //         let raw_bit = match bit {
        //             Bit::X => 0b00,
        //             Bit::Z => 0b01,
        //             Bit::Zero => 0b10,
        //             Bit::One => 0b11,
        //         };

        //         *byte |= raw_bit << (i * 2);
        //     }
        // }
        self.bits.write_bits(&mut b[header..header + bytes]);

        header + bytes
    }
}
impl<'a> ReadData<'a, StreamingValueChange> for StreamingVCBits<BitIter<'a>> {
    fn read_data(bits: usize, b: &'a [u8]) -> (Self, usize) {
        let (var_id, mut offset) = VarId::read_data((), b);
        let (offset_to_prev, size) = u64::read_data((), &b[offset..]);
        offset += size;
        let (timestamp_delta_index, size) = u64::read_data((), &b[offset..]);
        offset += size;

        let bytes = ((bits * 2) + 8 - 1) / 8;

        let data = StreamingVCBits {
            var_id,
            offset_to_prev,
            timestamp_delta_index,
            bits: BitIter::new(bits, &b[offset..offset + bytes]),
        };

        (data, offset + bytes)
    }
}

impl VariableLength for StreamingValueChange {
    type Meta = usize;
    type DefaultReadData = ();

    // #[inline]
    // fn from_bytes<'a, Data: ReadData<Self>>(bits: usize, b: &'a [u8]) -> (Data, usize) {
    //     let (var_id, mut offset) = <VarId as VariableLength>::from_bytes((), b);
    //     let (offset_to_prev, size) = <u64 as VariableLength>::from_bytes((), &b[offset..]);
    //     offset += size;
    //     let (timestamp_delta_index, size) = <u64 as VariableLength>::from_bytes((), &b[offset..]);
    //     offset += size;

    //     let bytes = ((bits * 2) + 8 - 1) / 8;

    //     let data = StreamingVCBits {
    //         var_id,
    //         offset_to_prev,
    //         timestamp_delta_index,
    //         bits: BitIter::new(bits, &b[offset..offset + bytes]),
    //     };

    //     (data, offset + bytes)
    // }
}

/// The variable id is the index of this in the `var_data` structure.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct StreamingVarMeta {
    var_id: VarId,
    bits: u32,
    last_value_change_offset: u64,
    number_of_value_changes: u64,
}

/// Hopefully this isn't a terrible idea.
unsafe impl Pod for Option<StreamingVarMeta> {}

/// Used to efficiently convert from a vcd that's larger than memory
/// to a structure that can be easily traversed in order to create a
/// db that can be easily and quickly searched.
pub struct StreamingDb {
    /// All var ids + some metadata, padded for each var_id to correspond exactly to its index in this array.
    var_data: MmapVec<Option<StreamingVarMeta>>,

    /// A list of timestamps, stored as the delta since the previous timestamp.
    timestamp_chain: VarMmapVec<u64>,

    value_change: VarMmapVec<StreamingValueChange>,
}

impl StreamingDb {
    pub fn load_vcd<R: Read>(parser: &mut Parser<R>) -> io::Result<Self> {
        let header = parser.parse_header()?;

        let var_tree = find_all_scopes_and_variables(header);

        let mut var_data = unsafe { MmapVec::create_with_capacity(var_tree.variables.len())? };

        let mut previous = 0;
        for (&var_id, var_info) in &var_tree.variables {
            while previous + 1 < var_id.get() {
                // Not contiguous, lets pad until it is.
                var_data.push(None);
                previous += 1;
            }
            previous = var_id.get();

            var_data.push(Some(StreamingVarMeta {
                var_id,
                bits: var_info.bits,
                last_value_change_offset: 0,
                number_of_value_changes: 0,
            }));
        }

        let mut timestamp_chain = unsafe { VarMmapVec::create()? };
        let mut value_change = unsafe { VarMmapVec::create()? };
        
        // for _ in 0..100 {
        //     println!("{:?}", parser.next().unwrap());
        // }

        let mut last_timestamp = 0;
        let mut timestamp_counter = 0;

        for command in parser {
            let command = command?;
            match command {
                Command::Timestamp(timestamp) => {
                    timestamp_chain.push((), timestamp - last_timestamp);
                    last_timestamp = timestamp;
                    timestamp_counter += 1;
                }
                Command::ChangeVector(code, values) => {
                    let var_id: VarId = code.number().try_into().unwrap();
                    let var_data = &mut var_data[var_id.get() as usize].unwrap();

                    var_data.last_value_change_offset = value_change.push(values.len(), StreamingVCBits {
                        var_id,
                        offset_to_prev: value_change.current_offset() - var_data.last_value_change_offset,
                        timestamp_delta_index: timestamp_counter - 1,
                        bits: values.into_iter().map(Into::into)
                    });
                    var_data.number_of_value_changes += 1;
                }
                Command::ChangeScalar(code, value) => {
                    let var_id: VarId = code.number().try_into().unwrap();
                    let var_data = &mut var_data[var_id.get() as usize].unwrap();

                    var_data.last_value_change_offset = value_change.push(1, StreamingVCBits {
                        var_id,
                        offset_to_prev: value_change.current_offset() - var_data.last_value_change_offset,
                        timestamp_delta_index: timestamp_counter - 1,
                        bits: Some(value.into()).into_iter(),
                    });
                    var_data.number_of_value_changes += 1;
                }
                _ => {},
            }
        }

        Ok(Self {
            var_data,
            timestamp_chain,
            value_change,
        })
    }
}

const NODE_CHILDREN: usize = 8;

/// While this is variable size (through the `VariableLength` trait),
/// each node for a given variable is the same size.
#[derive(Debug)]
struct Node<T: WriteBits> {
    /// Offsets (from offset of beginning of the tree this node is in) of this node's children.
    children: [u32; NODE_CHILDREN],
    averaged_bits: T,
}

enum NodeProxy {}

impl<T: WriteBits> WriteData<NodeProxy> for Node<T> {
    fn max_size(bits: usize) -> usize {
        let bytes = (bits + 8 - 1) / 8;
        mem::size_of::<[u32; NODE_CHILDREN]>() + bytes
    }

    fn write_bytes(&mut self, bits: usize, b: &mut [u8]) -> usize {
        let total_size = Self::max_size(bits);
        let children_size = mem::size_of::<[u32; NODE_CHILDREN]>();
        
        b[..children_size]
            .copy_from_slice(unsafe {
                slice::from_raw_parts(self.children.as_ptr() as *const u8, self.children.len() * mem::size_of::<u32>())
            });

        self.averaged_bits.write_bits(&mut b[children_size..total_size]);
        
        total_size
    }
}
impl<'a> ReadData<'a, NodeProxy> for Node<BitIter<'a>> {
    fn read_data(bits: usize, b: &'a [u8]) -> (Self, usize) {
        let total_size = Self::max_size(bits);
        let children_size = mem::size_of::<[u32; NODE_CHILDREN]>();

        let children = unsafe { *(b[children_size..].as_ptr() as *const [u32; NODE_CHILDREN]) };

        let node = Node {
            children,
            averaged_bits: BitIter::new(bits, &b[children_size..total_size])
        };

        (node, total_size)
    }
}

// struct JustBits<T: WriteBits>(T);
impl<T: WriteBits> WriteData<NodeProxy> for T {
    fn max_size(bits: usize) -> usize {
        (bits + 8 - 1) / 8
    }

    fn write_bytes(&mut self, bits: usize, b: &mut [u8]) -> usize {
        let bytes = (bits + 8 - 1) / 8;

        self.write_bits(&mut b[..bytes]);

        bytes
    }
}
impl<'a> ReadData<'a, NodeProxy> for BitIter<'a> {
    fn read_data(bits: usize, b: &'a [u8]) -> (Self, usize) {
        let bytes = Self::max_size(bits);
        (BitIter::new(bits, &b[..bytes]), bytes)
    }
}

impl VariableLength for NodeProxy {
    type Meta = usize;
    type DefaultReadData = ();

    // fn from_bytes<'a>(bits: usize, b: &'a [u8]) -> (Node<BitIter<'a>>, usize) {
    //     let total_size = Self::max_size(bits);
    //     let children_size = mem::size_of::<[u32; NODE_CHILDREN]>();

    //     let children = unsafe { *(b[children_size..].as_ptr() as *const [u32; NODE_CHILDREN]) };

    //     let node = Node {
    //         children,
    //         averaged_bits: BitIter::new(bits, &b[children_size..total_size])
    //     };

    //     (node, total_size)
    // }
}

/// Converts the number of value changes to the number of layers in the tree.
fn value_changes_to_layers(count: usize) -> usize {
    todo!()
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
struct FrustumTree {
    /// Contains a map of variable ids to offsets in the tree structure.
    offsets: HashMap<VarId, u64>,
    trees: VarMmapVec<NodeProxy>,
}

impl FrustumTree {
    pub fn generate(streaming_db: StreamingDb) -> Self {


        todo!()
    }
}

pub struct QueryDb {
    
    // frustum_tree: 
}
