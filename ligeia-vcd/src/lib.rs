#![feature(allocator_api)]

use std::{alloc::Allocator, collections::{BTreeMap}, convert::TryInto, fs::File, io::{self, Read}};

use fxhash::FxHashMap;
use ligeia_core::{Waves, WavesLoader, logic::Four, waves::{ChangeBlockList, ChangeHeader, Progress, ROOT_SCOPE, ScopeId, Scopes, Signedness, Storage, StorageId, Variable, VariableInterp}};
use vcd::{Header, IdCode, Parser, ScopeItem, Value, VarType};

pub struct VcdLoader;

impl<A: Allocator> WavesLoader<A> for VcdLoader {
    fn supports_file_extension(&self, s: &str) -> bool {
        matches!(s, "vcd")
    }

    fn description(&self) -> String {
        "Ligeia loader for VCD (Value Change Dump) files/streams".to_string()
    }

    fn load_file(
        &self,
        alloc: A,
        progress: &mut dyn FnMut(Progress, &Waves<A>),
        file: File,
    ) -> io::Result<Waves<A>> {
        let map = unsafe { mapr::Mmap::map(&file) };

        let waves = match map {
            Ok(map) => {
                let mut transient_command_count = 0;
                let total_len = map.len();

                let waves = load_vcd(alloc, &map[..], |map, waves| {
                    transient_command_count += 1;
                    if transient_command_count == 10_000 {
                        transient_command_count = 0;

                        progress(
                            Progress {
                                total: Some(total_len),
                                so_far: total_len - map.len(),
                            },
                            waves
                        );
                    }
                })?;

                waves
            }
            Err(_) => {
                println!("mmap failed, attempting to load file as a stream");

                let mut file = file;
                return self.load_stream(alloc, progress, &mut file);
            }
        };

        Ok(waves)
    }

    fn load_stream(
        &self,
        alloc: A,
        _progress: &mut dyn FnMut(Progress, &Waves<A>),
        reader: &mut dyn Read,
    ) -> io::Result<Waves<A>> {
        
        load_vcd(alloc, reader, |_, _| {})
    }
}

type StorageLookup = FxHashMap<IdCode, StorageId>;
type Storages = BTreeMap<StorageId, Storage>;

fn generate_scopes(header: &Header, change_block_list: &mut ChangeBlockList<impl Allocator>) -> (Scopes, StorageLookup, Storages) {
    fn recurse(
        scopes: &mut Scopes,
        storage_lookup: &mut StorageLookup,
        storages: &mut Storages,
        change_block_list: &mut ChangeBlockList<impl Allocator>,
        scope_gen: &mut impl FnMut() -> ScopeId,
        storage_gen: &mut impl FnMut() -> StorageId,
        items: &[ScopeItem],
        parent: ScopeId
    ) {
        for item in items {
            match item {
                ScopeItem::Scope(scope) => {
                    let id = scope_gen();
                    scopes.add_scope(parent, id, scope.identifier.clone()).unwrap();
                    recurse(
                        scopes,
                        storage_lookup,
                        storages,
                        change_block_list,
                        scope_gen,
                        storage_gen,
                        &scope.children,
                        id
                    );
                },
                ScopeItem::Var(var) => {
                    if !matches!(var.var_type, VarType::Wire | VarType::Reg | VarType::Integer) {
                        panic!("unsupported VCD variable type")
                    }

                    let storage_id = storage_gen();

                    let (msb, lsb) = var
                        .index
                        .map(|index| {
                            match index {
                                vcd::ReferenceIndex::BitSelect(bit) => (bit, bit),
                                vcd::ReferenceIndex::Range(msb, lsb) => (msb, lsb)
                            }
                        })
                        .unwrap_or_else(|| {
                            (
                                var.size.checked_sub(1).expect("variable size cannot be 0"),
                                0,
                            )
                        });

                    storage_lookup.insert(var.code, storage_id);
                    storages.insert(storage_id, Storage {
                        ty: ligeia_core::waves::StorageType::FourLogic,
                        width: var.size,
                        start: lsb,
                    });

                    change_block_list.add_storage::<Four>(storage_id, var.size as usize);

                    scopes.add_variable(parent, Variable {
                        name: var.reference.clone(),
                        interp: VariableInterp::Integer {
                            storages: vec![storage_id],
                            msb_index: msb,
                            lsb_index: 0,
                            signedness: Signedness::Unsigned,
                        },
                    }).unwrap();
                },
            }
        }
    }

    let mut id_inc = 0;
    let mut scope_gen = move || {
        id_inc += 1;
        ScopeId(id_inc)
    };
    let mut storage_inc = 0;
    let mut storage_gen = move || {
        let id = StorageId(storage_inc);
        storage_inc += 1;
        id
    };

    let mut storage_lookup =  FxHashMap::default();
    let mut storages = Storages::new();
    let mut scopes =  Scopes::new();

    recurse(
        &mut scopes,
        &mut storage_lookup,
        &mut storages,
        change_block_list,
        &mut scope_gen,
        &mut storage_gen,
        &header.items,
        ROOT_SCOPE,
    );

    (scopes, storage_lookup, storages)
}

fn load_vcd<A: Allocator, R: Read>(alloc: A, reader: R, mut after_every_cmd: impl FnMut(&R, &Waves<A>)) -> io::Result<Waves<A>> {
    let mut parser = Parser::new(reader);
    let header = parser.parse_header()?;
    
    let timescale = if let Some((timesteps, unit)) = header.timescale {
        timesteps as u128
            * match unit {
                vcd::TimescaleUnit::S => 1_000_000_000_000_000, // 1e15
                vcd::TimescaleUnit::MS => 1_000_000_000_000,    // 1e12
                vcd::TimescaleUnit::US => 1_000_000_000,        // 1e9
                vcd::TimescaleUnit::NS => 1_000_000,            // 1e6
                vcd::TimescaleUnit::PS => 1_000,
                vcd::TimescaleUnit::FS => 1,
            }
    } else {
        1
    };

    let mut changes = ChangeBlockList::new_in(alloc);
    
    let (scopes, storage_lookup, storages) = generate_scopes(&header, &mut changes);

    let mut waves = Waves {
        timescale,
        scopes,
        storages,
        changes,
        timestamps: vec![],
    };

    let mut current_timestamp_index = 0;

    loop {
        if let Some(command) = parser.next_command() {
            let command = command?;
    
            match command {
                vcd::Command::Timestamp(timestamp) => {
                    current_timestamp_index = waves.timestamps.len().try_into().unwrap();
                    waves.timestamps.push(timestamp);
                },
                vcd::Command::ChangeScalar(id_code, value) => {
                    let storage_id = storage_lookup[&id_code];
    
                    // VCDs are always four-valued logic
                    let logic = match value {
                        Value::V0 => Four::Zero,
                        Value::V1 => Four::One,
                        Value::X => Four::Unknown,
                        Value::Z => Four::HighImpedance,
                    };
    
                    let mut slice = unsafe {
                        waves.changes.push_get::<Four>(storage_id, ChangeHeader {
                            ts: current_timestamp_index,
                        })
                    };
    
                    slice.set(0, logic);
                }
                vcd::Command::ChangeVector(id_code, vector) => {
                    let storage_id = storage_lookup[&id_code];
    
                    let mut slice = unsafe {
                        waves.changes.push_get::<Four>(storage_id, ChangeHeader {
                            ts: current_timestamp_index,
                        })
                    };

                    assert_eq!(slice.width(), vector.len());
                    for (i, value) in vector.iter().enumerate() {
                        let logic = match value {
                            Value::V0 => Four::Zero,
                            Value::V1 => Four::One,
                            Value::X => Four::Unknown,
                            Value::Z => Four::HighImpedance,
                        };
    
                        slice.set(i, logic);
                    }
                }
                _ => {}
            }
        } else {
            break
        }

        after_every_cmd(parser.reader(), &waves);
    }

    Ok(waves)
}