use std::{convert::{TryFrom, TryInto}, fmt::{Debug, Display, Formatter}, fs::{File, OpenOptions}, io, marker::PhantomData, mem, num::NonZeroU64, ops::{Deref, DerefMut}, path::Path, slice};
use vcd::ScopeItem;

use crate::mmap_vec::{MmapVec, VariableLength, Pod, VarMmapVec};

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
                NonZeroU64::new_unchecked(x << 1)
            }))
        }
    }

    pub fn get(&self) -> u64 {
        self.0.get() >> 1
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

pub fn find_all_scopes_and_variables(header: vcd::Header) -> (Scope, Vec<VarId>) {
    fn recurse(ids: &mut Vec<VarId>, items: impl Iterator<Item=vcd::ScopeItem>) -> (Vec<Scope>, Vec<VarInfo>) {
        let mut scopes = vec![];
        let mut vars = vec![];

        for item in items {
            match item {
                ScopeItem::Var(var) => {
                    ids.push(var.code.number().try_into().unwrap());
                    vars.push(VarInfo {
                        name: var.reference,
                        id: var.code.number().try_into().unwrap(),
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

impl VariableLength for u64 {
    type Meta = ();

    #[inline]
    fn max_length(_: &()) -> usize {
        10
    }

    #[inline]
    fn write_bytes(&self, _: &(), mut b: &mut [u8]) -> usize {
        leb128::write::unsigned(&mut b, *self).unwrap()
    }

    #[inline]
    fn from_bytes(_: &(), b: &mut &[u8]) -> Self {
        leb128::read::unsigned(b).unwrap()
    }
}

impl VariableLength for VarId {
    type Meta = ();

    #[inline]
    fn max_length(_: &()) -> usize {
        10
    }

    #[inline]
    fn write_bytes(&self, _: &(), mut b: &mut [u8]) -> usize {
        leb128::write::unsigned(&mut b, self.0.get()).unwrap()
    }

    #[inline]
    fn from_bytes(_: &(), b: &mut &[u8]) -> Self {
        VarId(NonZeroU64::new(leb128::read::unsigned(b).unwrap()).unwrap())
    }
}

struct BitIter<'a> {
    bits: usize,
    index: usize,
    data: &'a [u8],
}

impl BitIter<'_> {
    pub fn num_bits(&self) -> usize {
        self.bits
    }
}

impl<'a> Iterator for BitIter<'a> {
    type Item = bool;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.bits {
            let byte = self.data[self.index / 8];
            let bit = byte & (1 << (self.index % 8)) != 0;
            self.index += 1;

            Some(bit)
        } else {
            None
        }
    }
}

struct StreamingValueChange<'a> {
    var_id: VarId,
    offset_to_prev: u64,
    bits: BitIter<'a>
}

impl<'a> VariableLength for StreamingValueChange<'a> {
    type Meta = usize;

    #[inline]
    fn max_length(bits: &usize) -> usize {
        <u64 as VariableLength>::max_length(&()) * 2 + (*bits / 8)
    }

    #[inline]
    fn write_bytes(&self, bits: &usize, mut b: &mut [u8]) -> usize {
        let header = <_ as VariableLength>::write_bytes(&self.var_id, &(), &mut b)
            + <_ as VariableLength>::write_bytes(&self.offset_to_prev, &(), &mut b);
        


        header + (*bits / 8)
    }

    #[inline]
    fn from_bytes(bits: &usize, b: &mut &[u8]) -> Self {
        let var_id = <_ as VariableLength>::from_bytes(&(), b);
        let offset_to_prev = <_ as VariableLength>::from_bytes(&(), b);

        Self {
            var_id,
            offset_to_prev,
        }
    }
}

/// The variable id is the index of this in the `var_data` structure.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct StreamingVarMeta {
    var_id: VarId,
    last_value_change_offset: Option<NonZeroU64>,
    number_of_value_changes: u64,
}

/// Hopefully this isn't a terrible idea.
unsafe impl Pod for Option<StreamingVarMeta> {}

/// Used to compactly convert from a vcd to a structure
/// that can be easily traversed in order to create a
/// db that can be easily and quickly searched.
struct StreamingDb {
    var_data: MmapVec<Option<StreamingVarMeta>>,
    timestamp_chain: VarMmapVec<u64>,
    value_change: VarMmapVec<StreamingValueChange>,
}
