use std::{convert::{TryFrom, TryInto}, fmt::{Debug, Display, Formatter}, fs::{File, OpenOptions}, io::{self, Read}, marker::PhantomData, mem, num::NonZeroU64, ops::{Deref, DerefMut}, path::Path, slice, unimplemented};
use vcd::{Command, Parser, ScopeItem, Value};

use crate::mmap_vec::{MmapVec, Pod, VarMmapVec, VariableLength, WriteData};

pub struct NotValidVarIdError(());

impl Debug for NotValidVarIdError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to convert to VarId")
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
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
    id: VarId,
}

#[derive(Debug)]
pub struct Scope {
    name: String,
    scopes: Vec<Scope>,
    vars: Vec<VarInfo>,
}

fn find_all_scopes_and_variables(header: vcd::Header) -> (Scope, Vec<VarId>) {
    fn recurse(ids: &mut Vec<VarId>, items: impl Iterator<Item=vcd::ScopeItem>) -> (Vec<Scope>, Vec<VarInfo>) {
        let mut scopes = vec![];
        let mut vars = vec![];

        for item in items {
            match item {
                ScopeItem::Var(var) => {
                    let id = var.code.number().try_into().unwrap();
                    ids.push(id);
                    vars.push(VarInfo {
                        name: var.reference,
                        id,
                    })
                }
                ScopeItem::Scope(scope) => {
                    let (sub_scopes, sub_vars) = recurse(ids, scope.children.into_iter());
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

    let mut ids = Vec::new();
    let (scopes, vars) = recurse(&mut ids, top_items.into_iter());

    ids.sort_unstable();
    ids.dedup();

    // INFO: Turns out the variable ids are usually sequential, but not always
    // let mut previous = vars[0].id;
    // for var in vars[1..].iter() {
    //     if var.id != previous + 1 {
    //         eprintln!("wasn't sequential at {}", var.id);
    //     }
    //     previous = var.id;
    // }
    
    (Scope {
        name, scopes, vars,
    }, ids)
}

impl WriteData for u64 {
    fn write_bytes(&mut self, _: &(), mut b: &mut [u8]) -> usize {
        leb128::write::unsigned(&mut b, *self).unwrap()
    }
}

impl VariableLength for u64 {
    type Meta = ();
    type ReadData<'a> = Self;

    #[inline]
    fn max_length(_: &()) -> usize {
        10
    }

    #[inline]
    fn write_bytes(mut data: impl WriteData<Self>, _: &(), b: &mut [u8]) -> usize {
        data.write_bytes(&(), b)
    }

    #[inline]
    fn from_bytes(_: &(), b: &mut &[u8]) -> Self {
        leb128::read::unsigned(b).unwrap()
    }
}

impl WriteData for VarId {
    fn write_bytes(&mut self, _: &(), mut b: &mut [u8]) -> usize {
        leb128::write::unsigned(&mut b, self.0.get()).unwrap()
    }
}

impl VariableLength for VarId {
    type Meta = ();
    type ReadData<'a> = Self;

    #[inline]
    fn max_length(_: &()) -> usize {
        10
    }

    #[inline]
    fn write_bytes(mut data: impl WriteData<Self>, _: &(), b: &mut [u8]) -> usize {
        data.write_bytes(&(), b)
    }

    #[inline]
    fn from_bytes(_: &(), b: &mut &[u8]) -> Self {
        VarId(NonZeroU64::new(leb128::read::unsigned(b).unwrap()).unwrap())
    }
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

impl<T: Iterator<Item = Bit>> WriteData<StreamingValueChange> for StreamingVCBits<T> {
    fn write_bytes(&mut self, bits: &usize, mut b: &mut [u8]) -> usize {
        let mut header = <VarId as VariableLength>::write_bytes(self.var_id, &(), &mut b);
        header += <u64 as VariableLength>::write_bytes(self.offset_to_prev, &(), &mut b[header..]);
        header += <u64 as VariableLength>::write_bytes(self.timestamp_delta_index, &(), &mut b[header..]);

        let bytes = (*bits + 8 - 1) / 8;

        for byte in b[header..header + bytes].iter_mut() {
            for (i, bit) in (&mut self.bits).take(4).enumerate() {
                let raw_bit = match bit {
                    Bit::X => 0b00,
                    Bit::Z => 0b01,
                    Bit::Zero => 0b10,
                    Bit::One => 0b11,
                };

                *byte |= raw_bit << (i * 2);
            }
        }

        header + bytes
    }
}

pub struct StreamingValueChange(());

#[derive(Debug)]
pub struct StreamingVCBits<T> {
    pub var_id: VarId,
    pub offset_to_prev: u64,
    pub timestamp_delta_index: u64,
    pub bits: T,
}

impl VariableLength for StreamingValueChange {
    type Meta = usize;
    // type WriteData<'a> = StreamingVCBits<impl Iterator<Item = Bit>>;
    type ReadData<'a> = StreamingVCBits<BitIter<'a>>;

    #[inline]
    fn max_length(bits: &usize) -> usize {
        <u64 as VariableLength>::max_length(&()) * 3 + ((*bits + 8 - 1) / 8)
    }

    #[inline]
    fn write_bytes(mut data: impl WriteData<Self>, bits: &usize, b: &mut [u8]) -> usize {
        data.write_bytes(bits, b)
    }

    #[inline]
    fn from_bytes<'a>(bits: &usize, b: &mut &'a [u8]) -> StreamingVCBits<BitIter<'a>> {
        let var_id = <VarId as VariableLength>::from_bytes(&(), b);
        let offset_to_prev = <u64 as VariableLength>::from_bytes(&(), b);
        let timestamp_delta_index = <u64 as VariableLength>::from_bytes(&(), b);
        let bytes = ((*bits * 2) + 8 - 1) / 8;

        StreamingVCBits {
            var_id,
            offset_to_prev,
            timestamp_delta_index,
            bits: BitIter::new(*bits, &b[..bytes]),
        }
    }
}

/// The variable id is the index of this in the `var_data` structure.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct StreamingVarMeta {
    var_id: VarId,
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

        let (tree, vars) = find_all_scopes_and_variables(header);

        let mut var_data = unsafe { MmapVec::create_with_capacity(vars.len())? };

        let mut previous = 0;
        for var_id in vars {
            while previous + 1 < var_id.get() {
                // Not contiguous, lets pad until it is.
                var_data.push(None);
                previous += 1;
            }
            previous = var_id.get();

            var_data.push(Some(StreamingVarMeta {
                var_id,
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
                    timestamp_chain.push(&(), timestamp - last_timestamp);
                    last_timestamp = timestamp;
                    timestamp_counter += 1;
                }
                Command::ChangeVector(code, values) => {
                    let var_id: VarId = code.number().try_into().unwrap();
                    let var_data = &mut var_data[var_id.get() as usize].unwrap();

                    var_data.last_value_change_offset = value_change.push(&values.len(), StreamingVCBits {
                        var_id,
                        offset_to_prev: value_change.current_offset() - var_data.last_value_change_offset,
                        timestamp_delta_index: timestamp_counter - 1,
                        bits: values.into_iter().map(Into::into)
                    });
                }
                Command::ChangeScalar(code, value) => {
                    let var_id: VarId = code.number().try_into().unwrap();
                    let var_data = &mut var_data[var_id.get() as usize].unwrap();

                    var_data.last_value_change_offset = value_change.push(&1, StreamingVCBits {
                        var_id,
                        offset_to_prev: value_change.current_offset() - var_data.last_value_change_offset,
                        timestamp_delta_index: timestamp_counter - 1,
                        bits: Some(value.into()).into_iter(),
                    });
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
