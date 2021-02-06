// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{future::Future, io::Read, path::Path, sync::Arc};
use crate::types::{BitSlice, BitVec, QitSlice};

#[derive(Debug)]
pub struct Scope {
    pub name: String,
    pub variables: Vec<Variable>,
    pub scopes: Vec<Scope>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum VariableInfo {
    Integer {
        bits: usize,
        is_signed: bool,
    },
    Enum {
        bits: usize,
        fields: Vec<(String, BitVec)>,
    },
    String {
        len: usize,
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VariableId(usize);

#[derive(Debug)]
pub struct Variable {
    pub id: VariableId,
    pub name: String,
    pub info: VariableInfo,
}

/// A waveform database does not necessarily have any variables
/// immediately accessible, but they can be queried for to load
/// them individually.
pub trait WaveformDatabase: Send {
    /// Femtoseconds per timestep
    fn timescale(&self) -> u128;
    /// Retrieve the variable-scope tree of the database.
    fn tree(&self) -> Arc<[Scope]>;
    /// Load one variable asyncronously.
    fn load_waveform(&self, id: VariableId) -> Box<dyn Future<Output = Waveform>>;
}

pub trait WaveformLoader: Sync {
    fn supports_file_extension(&self, s: &str) -> bool;
    fn description(&self) -> String;

    /// A file is technically a stream, but generally, specializing parsers for files can be more efficient than parsing
    /// from a generic reader.
    fn load_file(&self, path: &Path) -> anyhow::Result<Box<dyn WaveformDatabase>>;
    fn load_stream(&self, reader: &mut dyn Read) -> anyhow::Result<Box<dyn WaveformDatabase>>;
}

pub enum Waveform {
    Binary(BitWaveform),
    Quaternary(QitWaveform),
    Utf8()
}

pub struct BitWaveform {
    pub layers: Vec<Box<dyn BitLayer + Send>>,
}

pub trait BitLayer {
    fn iter(&self) -> Box<dyn Iterator<Item = (u64, BitSlice)>>;
    fn len(&self) -> usize;
}

pub struct QitWaveform {
    pub layers: Vec<Box<dyn QitLayer + Send>>,
}

pub trait QitLayer {
    fn iter(&self) -> Box<dyn Iterator<Item = (u64, QitSlice)>>;
    fn len(&self) -> usize;
}

