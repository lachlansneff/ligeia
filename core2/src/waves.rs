use std::{alloc::Allocator, fs::File, io::Read};
use crate::unit::TwoLogic;

mod change;

pub use self::change::{ChangeBlockList, StorageIter, ChangeOffset, ChangeHeader};

#[derive(Clone, Copy)]
pub enum StorageType {
    TwoLogic,
    FourLogic,
    NineLogic,
}

pub struct Storage {
    pub id: u32,
    pub ty: StorageType,
    pub width: u32,
    pub start: u32,
}

pub struct EnumSpec {
    pub name: String,
    pub value: Vec<TwoLogic>,
}

#[derive(Clone, Copy)]
pub enum Signedness {
    SignedTwosComplement,
    Unsigned,
}

pub enum VariableInterp {
    None,
    Integer {
        storage_ids: Vec<u32>,
        msb_index: u32,
        lsb_index: u32,
        signedness: Signedness,
    },
    Enum {
        storage_id: u32,
        specs: Vec<EnumSpec>,
    },
    Utf8 {
        storage_id: u32,
    },
}

pub struct Variable {
    pub scope_id: u32,
    pub name: String,
    pub interp: VariableInterp,
}

pub struct Scope {
    pub parent_scope_id: u32,
    pub this_scope_id: u32,
    pub name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Total bytes in the file/stream, if known.
    pub total: Option<usize>,
    /// Bytes processed so far.
    pub so_far: usize,
}

pub trait WavesLoader<A: Allocator + Clone> {
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
    /// The first index is the top-level parent scope and is always present.
    pub scopes: Vec<Scope>,
    pub variables: Vec<Variable>,
    pub changes: ChangeBlockList<A>,
}
