// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{collections::{BTreeMap, HashMap}, convert::{TryFrom, TryInto}, fmt::{Debug, Display, Formatter}, fs::File, future::Future, io::{self, Read}, iter, mem, num::NonZeroU64, sync::Arc, time::Instant};

use anyhow::anyhow;
use io::BufReader;
use vcd::{Command, Parser, ScopeItem};

use crate::{db::{WaveformDatabase, WaveformLoader}, mmap_vec::{ConsistentSize, ReallocDisabled, VarMmapVec, VarVecWindow, VariableLength, VariableRead, VariableWrite}, types::{Qit, QitSlice, QitVec}};

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

impl VariableWrite for VarId {
    #[inline]
    fn max_size(_: ()) -> usize {
        10
    }

    fn write_variable(self, _: (), b: &mut [u8]) -> usize {
        // leb128::write::unsigned(&mut b, self.0.get()).unwrap()
        varint_simd::encode_to_slice(self.0.get(), b) as usize
    }
}
impl VariableRead<'_> for VarId {
    fn read_variable(_: (), b: &[u8]) -> (Self, usize) {
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

impl<T: WriteQits> VariableWrite<ValueChangeProxy> for ValueChange<T> {
    #[inline]
    fn max_size(qits: usize) -> usize {
        <u64 as VariableWrite>::max_size(()) * 3 + Qit::bits_to_bytes(qits)
    }

    fn write_variable(self, qits: usize, mut b: &mut [u8]) -> usize {
        let mut header = self.var_id.write_variable((), &mut b);
        header += self.offset_to_prev.write_variable((), &mut b[header..]);
        header += self
            .offset_to_prev_timestamp
            .write_variable((), &mut b[header..]);

        let bytes = Qit::bits_to_bytes(qits);

        self.qits.write_qits(&mut b[header..header + bytes]);

        header + bytes
    }
}
impl<'a> VariableRead<'a, ValueChangeProxy> for ValueChange<QitSlice<'a>> {
    fn read_variable(qits: usize, b: &'a [u8]) -> (Self, usize) {
        let (var_id, mut offset) = VarId::read_variable((), b);
        let (offset_to_prev, size) = u64::read_variable((), &b[offset..]);
        offset += size;
        let (offset_to_prev_timestamp, size) = u64::read_variable((), &b[offset..]);
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
    timescale: Option<(u32, vcd::TimescaleUnit)>,
}

impl VcdConverter {
    fn load_vcd<R: Read>(reader: R) -> io::Result<Self> {
        let mut parser = Parser::new(reader);
        let header = parser.parse_header()?;

        let timescale = header.timescale;
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
            timescale,
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

    /// Single-threaded for now, but this is probably possible to multi-thread.
    fn into_db(self) -> io::Result<VcdDb> {
        let layer_sizer = |var_meta: &VarMeta| {
            let value_size = Node::size(var_meta.qits as usize);
            let mut count_on_layer = var_meta.number_of_value_changes as usize;
            let mut finished_next = false;

            iter::from_fn(move || {
                let ret = if finished_next {
                    None
                } else {
                    Some((count_on_layer, count_on_layer * value_size))
                };

                if count_on_layer < 1024 {
                    finished_next = true;
                }

                count_on_layer /= 4;

                ret
            })
        };

        let total_size: usize = self.var_metas.values().map(|var_meta| -> usize {
            layer_sizer(var_meta).map(|(_, size)| size).sum()
        }).sum();

        println!("test total size: {} bytes", total_size);

        let start = Instant::now();

        // This is a bad abstraction for this purpose. Needs a rework with the ability to get windows
        // into the forest.
        let mut forest: VarMmapVec<Node, ReallocDisabled> = VarMmapVec::create_with_capacity(total_size)?;
        let mut offsets: HashMap<VarId, usize, ahash::RandomState> = HashMap::default();
        let mut window = forest.window(..);

        let mut times = vec![];

        for (&var_id, var_meta) in self.var_metas.iter() {
            let start = Instant::now();
            // this iterator is in reverse, so we need to be careful to lay them down *not* in reverse.
            let value_change_iter = self.iter_reverse_value_change(var_id);
            let mut layer_sizes = layer_sizer(var_meta);

            let (_, bottom_layer_size) = layer_sizes.next().unwrap();
            let mut layer_window = window.take_window(bottom_layer_size);
            offsets.insert(var_id, layer_window.offset_in_mapping());

            for (timestamp, value_change) in value_change_iter {
                layer_window.push_rev(var_meta.qits as _, Node {
                    timestamp,
                    value_change,
                });
            }

            let previous_layer_window = layer_window;

            let mut value_change_mixed = QitVec::empty(var_meta.qits as _);

            // The rest of the layers
            // TODO: Fill in "mipmapping".
            for (layer_len, layer_size) in layer_sizes {
                let mut layer_window = window.take_window(layer_size);
                let mut previous_layer_iter = previous_layer_window.iter(var_meta.qits as _);

                for _ in 0..layer_len {
                    let mut mixed_timestamp: u128 = 0;
                    for (i, Node { timestamp, value_change }) in (&mut previous_layer_iter).take(4).enumerate() {
                        if i == 0 {
                            mixed_timestamp = timestamp as _;
                            value_change_mixed.mix(value_change, |_, qit| qit);
                        } else {
                            mixed_timestamp += timestamp as u128;
                            value_change_mixed.mix(value_change, |vqit, sqit| {
                                // Sort of "or"
                                match (vqit, sqit) {
                                    (Qit::Zero, Qit::Zero) => Qit::Zero,
                                    (Qit::One, Qit::Zero) => Qit::One,
                                    (Qit::Zero, Qit::One) => Qit::One,
                                    (Qit::One, Qit::One) => Qit::One,
                                    (Qit::Z, _) => Qit::Z,
                                    (_, Qit::Z) => Qit::Z,
                                    (Qit::X, _) => Qit::X,
                                    (_, Qit::X) => Qit::X,
                                }
                            })
                        }
                    }

                    layer_window.push(var_meta.qits as _, Node {
                        timestamp: (mixed_timestamp / 4) as _,
                        value_change: value_change_mixed.as_slice(),
                    });
                }
            }

            times.push((var_id, var_meta.number_of_value_changes, start.elapsed()));
        }

        let (var_id, number_of_value_changes, max_time) = times.iter().max_by_key(|(_, _, time)| time).unwrap();

        println!("variable {}, with {} value changes, took {:.2} ms", var_id, number_of_value_changes, max_time.as_secs_f32() * 1000.);

        let elapsed = start.elapsed();
        println!("writing out bottom layers took: {:.2} seconds", elapsed.as_secs_f32());
        
        let timescale = if let Some((timesteps, unit)) = self.timescale {
            timesteps as u128 * match unit {
                vcd::TimescaleUnit::S => 1_000_000_000_000_000, // 1e15
                vcd::TimescaleUnit::MS => 1_000_000_000_000, // 1e12
                vcd::TimescaleUnit::US => 1_000_000_000, // 1e9
                vcd::TimescaleUnit::NS => 1_000_000, // 1e6
                vcd::TimescaleUnit::PS => 1_000,
                vcd::TimescaleUnit::FS => 1,
            }
        } else {
            1
        };

        Ok(VcdDb {
            timescale,
        })
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

struct Node<'a> {
    timestamp: u64,
    value_change: QitSlice<'a>,
}

impl<'a, 'b> VariableWrite<Node<'a>> for Node<'b> {
    fn max_size(qits: usize) -> usize {
        mem::size_of::<u64>() + Qit::bits_to_bytes(qits)
    }

    fn write_variable(self, qits: usize, b: &mut [u8]) -> usize {
        let timestamp_offset = mem::size_of::<u64>();
        b[..timestamp_offset].copy_from_slice(&self.timestamp.to_le_bytes());

        let bytes = Qit::bits_to_bytes(qits);
        self.value_change.write_qits(&mut b[timestamp_offset..timestamp_offset + bytes]);

        bytes
    }
}
impl<'a> VariableRead<'a> for Node<'a> {
    fn read_variable(qits: usize, b: &'a [u8]) -> (Self, usize) {
        let timestamp_offset = mem::size_of::<u64>();
        let timestamp = u64::from_le_bytes(b[..timestamp_offset].try_into().unwrap());

        let bytes = Qit::bits_to_bytes(qits);
        let node = Node {
            timestamp,
            value_change: QitSlice::new(qits, &b[timestamp_offset..timestamp_offset + bytes]),
        };

        (node, timestamp_offset + bytes)
    }
}

impl VariableLength for Node<'_> {
    type Meta = usize;
    type DefaultReadData = ();
}

impl ConsistentSize for Node<'_> {
    fn size(qits: usize) -> usize {
        mem::size_of::<u64>() + Qit::bits_to_bytes(qits)
    }
}

struct VcdDb {
    /// Femtoseconds per timestep
    timescale: u128,
}

impl WaveformDatabase for VcdDb {
    fn timescale(&self) -> u128 {
        self.timescale
    }

    fn tree(&self) -> Arc<[crate::db::Scope]> {
        todo!()
    }

    fn load_waveform(&self, id: crate::db::VariableId) -> Box<dyn Future<Output = crate::db::Waveform>> {
        todo!()
    }
}

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
        let mut f = File::open(&path)?;
        let map = unsafe { mapr::Mmap::map(&f) };

        // let converter = VcdConverter::load_vcd(&map[..])?;
        let converter = match map {
            Ok(map) => VcdConverter::load_vcd(&map[..])?,
            Err(_) => {
                println!("mmap failed, attempting to load file as a stream");

                // VcdConverter::load_vcd(BufReader::with_capacity(1_000_000, f))?
                return self.load_stream(&mut f);
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

        let db = converter.into_db()?;

        Err(anyhow!("not yet implemented"))
    }

    fn load_stream(
        &self,
        reader: &mut dyn Read,
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
