use crate::logic::{self, LogicArray};
use std::collections::BTreeMap;
use std::{alloc::Allocator, fs::File, io::Read};

mod change;
mod scope;

pub use self::change::{ChangeBlockList, ChangeHeader, ChangeOffset, StorageIter};
pub use self::scope::{Scope, Scopes, ScopesError, ROOT_SCOPE};

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageId(pub u32);

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScopeId(pub u32);

#[derive(Clone, Copy)]
pub enum StorageType {
    TwoLogic,
    FourLogic,
    NineLogic,
}

pub struct Storage {
    pub ty: StorageType,
    pub width: u32,
    pub start: u32,
}

pub struct EnumSpec {
    pub name: String,
    pub value: LogicArray<logic::Two>,
}

#[derive(Clone, Copy)]
pub enum Signedness {
    SignedTwosComplement,
    Unsigned,
}

pub enum VariableInterp {
    None,
    Integer {
        storages: Vec<StorageId>,
        msb_index: u32,
        lsb_index: u32,
        signedness: Signedness,
    },
    Enum {
        storage: StorageId,
        specs: Vec<EnumSpec>,
    },
    Utf8 {
        storage: StorageId,
    },
}

pub struct Variable {
    pub name: String,
    pub interp: VariableInterp,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Total bytes in the file/stream, if known.
    pub total: Option<usize>,
    /// Bytes processed so far.
    pub so_far: usize,
}

pub trait WavesLoader<A: Allocator> {
    fn supports_file_extension(&self, s: &str) -> bool;
    fn description(&self) -> String;

    /// A file is technically a stream, but generally, specializing parsers for files can be more efficient than parsing
    /// from a generic reader.
    fn load_file(
        &self,
        alloc: A,
        progress: &mut dyn FnMut(Progress, &Waves<A>),
        file: File,
    ) -> Result<Waves<A>, String>;

    fn load_stream(
        &self,
        alloc: A,
        progress: &mut dyn FnMut(Progress, &Waves<A>),
        reader: &mut dyn Read,
    ) -> Result<Waves<A>, String>;
}

pub struct Waves<A: Allocator> {
    pub scopes: Scopes,
    pub storages: BTreeMap<StorageId, Storage>,
    pub changes: ChangeBlockList<A>,
}
