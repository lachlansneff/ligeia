// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap},
    convert::{TryFrom, TryInto},
    fmt::{Debug, Display, Formatter},
    fs::File,
    io::{self, Read},
    num::NonZeroU64,
    time::Instant,
};

use anyhow::anyhow;
use io::BufReader;
use vcd::{Command, Parser, ScopeItem};

use crate::{db::{WaveformDatabase, WaveformLoader}, mmap_vec::{ReadData, VarMmapVec, VariableLength, WriteData}, types::{Qit, QitSlice}};

struct NotValidVarIdError(());

impl Debug for NotValidVarIdError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to convert to VarId")
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct VarId(NonZeroU64);

impl VarId {
    pub fn new(x: u64) -> Result<Self, NotValidVarIdError> {
        if x == u64::max_value() {
            Err(NotValidVarIdError(()))
        } else {
            Ok(Self(unsafe { NonZeroU64::new_unchecked(x + 1) }))
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

impl From<vcd::Value> for Qit {
    fn from(v: vcd::Value) -> Self {
        match v {
            vcd::Value::X => Qit::X,
            vcd::Value::Z => Qit::Z,
            vcd::Value::V0 => Qit::Zero,
            vcd::Value::V1 => Qit::One,
        }
    }
}

#[derive(Debug)]
struct VarInfo {
    pub name: String,
    pub qits: u32,
}

#[derive(Debug)]
struct Scope {
    pub name: String,
    pub scopes: Vec<Scope>,
    pub vars: Vec<VarId>,
}

struct VarTree {
    pub scope: Scope,
    pub variables: BTreeMap<VarId, VarInfo>,
}

fn find_all_scopes_and_variables(header: vcd::Header) -> VarTree {
    fn recurse(
        variables: &mut BTreeMap<VarId, VarInfo>,
        items: impl Iterator<Item = vcd::ScopeItem>,
    ) -> (Vec<Scope>, Vec<VarId>) {
        let mut scopes = vec![];
        let mut vars = vec![];

        for item in items {
            match item {
                ScopeItem::Var(var) => {
                    let id = var.code.number().try_into().unwrap();
                    vars.push(id);
                    variables.insert(
                        id,
                        VarInfo {
                            name: var.reference,
                            qits: var.size,
                        },
                    );
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

trait WriteQits: IntoIterator<Item = Qit> {
    fn write_qits(self, bytes: &mut [u8]);
}

impl<T: IntoIterator<Item = Qit>> WriteQits for T {
    #[inline]
    fn write_qits(self, bytes: &mut [u8]) {
        let mut iter = self.into_iter();
        for byte in bytes.iter_mut() {
            for (i, qit) in (&mut iter).take(4).enumerate() {
                let raw_qit = match qit {
                    Qit::Zero => 0,
                    Qit::One => 1,
                    Qit::X => 2,
                    Qit::Z => 3,
                };

                *byte |= raw_qit << (i * 2);
            }
        }
    }
}

enum ValueChangeProxy {}

#[derive(Debug)]
struct ValueChange<T: WriteQits> {
    var_id: VarId,
    offset_to_prev: u64,
    offset_to_prev_timestamp: u64,
    qits: T,
}

impl<T: WriteQits> WriteData<ValueChangeProxy> for ValueChange<T> {
    #[inline]
    fn max_size(qits: usize) -> usize {
        <u64 as WriteData>::max_size(()) * 3 + Qit::bits_to_bytes(qits)
    }

    fn write_bytes(self, qits: usize, mut b: &mut [u8]) -> usize {
        let mut header = self.var_id.write_bytes((), &mut b);
        header += self.offset_to_prev.write_bytes((), &mut b[header..]);
        header += self
            .offset_to_prev_timestamp
            .write_bytes((), &mut b[header..]);

        let bytes = Qit::bits_to_bytes(qits);

        self.qits.write_qits(&mut b[header..header + bytes]);

        header + bytes
    }
}
impl<'a> ReadData<'a, ValueChangeProxy> for ValueChange<QitSlice<'a>> {
    fn read_data(qits: usize, b: &'a [u8]) -> (Self, usize) {
        let (var_id, mut offset) = VarId::read_data((), b);
        let (offset_to_prev, size) = u64::read_data((), &b[offset..]);
        offset += size;
        let (offset_to_prev_timestamp, size) = u64::read_data((), &b[offset..]);
        offset += size;

        let bytes = Qit::bits_to_bytes(qits);

        let data = ValueChange {
            var_id,
            offset_to_prev,
            offset_to_prev_timestamp,
            qits: QitSlice::new(qits, &b[offset..offset + bytes]),
        };

        (data, offset + bytes)
    }
}

impl VariableLength for ValueChangeProxy {
    type Meta = usize;
    type DefaultReadData = ();
}

/// The variable id is the index of this in the `var_data` structure.
#[derive(Clone)]
struct VarMeta {
    // var_id: VarId,
    qits: u32,
    last_value_change_offset: u64,
    number_of_value_changes: u64,
    last_timestamp_offset: u64,
    last_timestamp: u64,
}

/// Used to efficiently convert from a vcd that's larger than memory
/// to a structure that can be easily traversed in order to create a
/// db that can be easily and quickly queried.
struct VcdConverter {
    // All storages.
    var_metas: HashMap<VarId, VarMeta, ahash::RandomState>,

    /// A list of timestamps, stored as the delta since the previous timestamp.
    timestamp_chain: VarMmapVec<u64>,

    value_change: VarMmapVec<ValueChangeProxy>,

    var_tree: VarTree,
}

impl VcdConverter {
    fn load_vcd<R: Read>(reader: R) -> io::Result<Self> {
        let mut parser = Parser::new(reader);
        let header = parser.parse_header()?;

        let var_tree = find_all_scopes_and_variables(header);

        let mut var_metas = HashMap::with_capacity_and_hasher(
            var_tree.variables.len(),
            ahash::RandomState::default(),
        );

        for (&var_id, var_info) in &var_tree.variables {
            var_metas.insert(
                var_id,
                VarMeta {
                    qits: var_info.qits,
                    last_value_change_offset: 0,
                    number_of_value_changes: 0,
                    last_timestamp_offset: 0,
                    last_timestamp: 0,
                },
            );
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
                    let var_meta = var_metas.get_mut(&var_id).unwrap();

                    var_meta.last_value_change_offset = value_change.push(
                        values.len(),
                        ValueChange {
                            var_id,
                            offset_to_prev: value_change.current_offset()
                                - var_meta.last_value_change_offset,
                            offset_to_prev_timestamp: timestamp_offset
                                - var_meta.last_timestamp_offset,
                            qits: values.into_iter().copied().map(Into::into),
                        },
                    );
                    var_meta.number_of_value_changes += 1;
                    var_meta.last_timestamp_offset = timestamp_offset;
                    var_meta.last_timestamp = last_timestamp;

                    processed_commands_count += 1;
                }
                Command::ChangeScalar(code, value) => {
                    let var_id: VarId = code.number().try_into().unwrap();
                    let var_meta = var_metas.get_mut(&var_id).unwrap();

                    var_meta.last_value_change_offset = value_change.push(
                        1,
                        ValueChange {
                            var_id,
                            offset_to_prev: value_change.current_offset()
                                - var_meta.last_value_change_offset,
                            offset_to_prev_timestamp: timestamp_offset
                                - var_meta.last_timestamp_offset,
                            qits: Some(value.into()).into_iter(),
                        },
                    );
                    var_meta.number_of_value_changes += 1;
                    var_meta.last_timestamp_offset = timestamp_offset;
                    var_meta.last_timestamp = last_timestamp;

                    processed_commands_count += 1;
                }
                _ => {}
            }
        }

        let elapsed = start.elapsed();
        println!(
            "processed {} commands in {:.2} seconds",
            processed_commands_count,
            elapsed.as_secs_f32()
        );
        println!("contained {} timestamps", number_of_timestamps);
        println!("last timestamp: {}", last_timestamp);

        Ok(Self {
            var_metas,
            timestamp_chain,
            value_change,
            var_tree,
        })
    }

    /// Iterates backward through the value changes for a specific variable.
    fn iter_reverse_value_change(&self, var_id: VarId) -> ReverseValueChangeIter {
        let var_meta = &self.var_metas[&var_id];

        ReverseValueChangeIter {
            value_changes: &self.value_change,
            timestamp_chain: &self.timestamp_chain,
            current_timestamp: var_meta.last_timestamp,
            current_timestamp_offset: var_meta.last_timestamp_offset,
            current_value_change_offset: var_meta.last_value_change_offset,
            remaining: var_meta.number_of_value_changes,
            qits: var_meta.qits as usize,
        }
    }
}

/// Iterates backward through the value changes for a specific variable.
struct ReverseValueChangeIter<'a> {
    value_changes: &'a VarMmapVec<ValueChangeProxy>,
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

        let timestamp_delta: u64 = self
            .timestamp_chain
            .get_at((), self.current_timestamp_offset);
        let timestamp = self.current_timestamp;
        self.current_timestamp -= timestamp_delta;

        let value_change: ValueChange<QitSlice<'a>> = self
            .value_changes
            .get_at(self.qits, self.current_value_change_offset);

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

struct VcdDb {}

pub struct VcdLoader {}

impl VcdLoader {
    pub const fn new() -> Self {
        Self {}
    }
}

impl WaveformLoader for VcdLoader {
    fn supports_file_extension(&self, s: &str) -> bool {
        matches!(s, "vcd")
    }

    fn description(&self) -> String {
        "the Value Change Dump (VCD) loader".to_string()
    }

    fn load_file(
        &self,
        path: &std::path::Path,
    ) -> anyhow::Result<Box<dyn WaveformDatabase>> {
        let f = File::open(&path)?;
        let map = unsafe { mapr::Mmap::map(&f) };

        // let converter = VcdConverter::load_vcd(&map[..])?;
        let converter = match map {
            Ok(map) => VcdConverter::load_vcd(&map[..])?,
            Err(_) => {
                println!("mmap failed, attempting to load file as a stream");

                // VcdConverter::load_vcd(BufReader::with_capacity(1_000_000, f))?
                return self.load_stream(Box::new(f));
            }
        };

        println!("contains {} variables", converter.var_tree.variables.len());
        let (&example_var_id, var_info) = converter.var_tree.variables.iter().nth(4).unwrap();
        let mut reverse_value_changes = converter.iter_reverse_value_change(example_var_id);
        println!(
            "variable \"{}\" ({}) has {} value changes",
            var_info.name,
            example_var_id,
            reverse_value_changes.len()
        );
        println!(
            "last value change: {:?}",
            reverse_value_changes.next().unwrap()
        );
        println!(
            "second to last value change: {:?}",
            reverse_value_changes.next().unwrap()
        );
        println!(
            "third to last value change: {:?}",
            reverse_value_changes.next().unwrap()
        );

        Err(anyhow!("not yet implemented"))
    }

    fn load_stream(
        &self,
        reader: Box<dyn Read + '_>,
    ) -> anyhow::Result<Box<dyn WaveformDatabase>> {
        let converter = VcdConverter::load_vcd(BufReader::with_capacity(1_000_000, reader))?;

        println!("contains {} variables", converter.var_tree.variables.len());
        let (&example_var_id, var_info) = converter.var_tree.variables.iter().nth(4).unwrap();
        let mut reverse_value_changes = converter.iter_reverse_value_change(example_var_id);
        println!(
            "variable \"{}\" ({}) has {} value changes",
            var_info.name,
            example_var_id,
            reverse_value_changes.len()
        );
        println!(
            "last value change: {:?}",
            reverse_value_changes.next().unwrap()
        );
        println!(
            "second to last value change: {:?}",
            reverse_value_changes.next().unwrap()
        );
        println!(
            "third to last value change: {:?}",
            reverse_value_changes.next().unwrap()
        );

        Err(anyhow!("not yet implemented"))
    }
}
