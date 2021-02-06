use std::{collections::HashMap, fs::File, io::{self, BufReader, Read}, iter, time::Instant};
use vcd::{Command, Parser, ScopeItem};
use anyhow::anyhow;

use crate::{db::{Scope, Variable, VariableId, VariableInfo, WaveformDatabase, WaveformLoader}, mmap_alloc::MmappableAllocator, unsized_types::{KnownUnsizedVec, Qit, ValueChangeNode}};

fn traverse_scopes(header: vcd::Header) -> (Scope, HashMap<VariableId, usize>) {
    fn recurse(childen: impl Iterator<Item = vcd::ScopeItem>, size_map: &mut HashMap<VariableId, usize>) -> (Vec<Variable>, Vec<Scope>) {
        let mut variables = vec![];
        let mut scopes = vec![];

        for item in childen {
            match item {
                ScopeItem::Scope(mut scope) => {
                    let (variables, new_scopes) = recurse(scope.children.drain(..), size_map);
                    scopes.push(Scope {
                        name: scope.identifier,
                        variables,
                        scopes: new_scopes,
                    });
                }
                ScopeItem::Var(var) => {
                    let id = VariableId(var.code.number());

                    size_map.insert(id, var.size as usize);

                    let info = match var.var_type {
                        vcd::VarType::Integer | vcd::VarType::Wire | vcd::VarType::Reg => VariableInfo::Integer {
                            bits: var.size as usize,
                            is_signed: false,
                        },
                        vcd::VarType::String => VariableInfo::String { len: var.size as usize },
                        _ => {
                            println!("var {:?} has unsupported type {:?}", var.reference, var.var_type);
                            continue;
                        }
                    };
                    
                    variables.push(Variable {
                        id,
                        name: var.reference,
                        info,
                    });
                }
            }
        }

        (variables, scopes)
    }

    let mut size_map = HashMap::new();

    let (variables, scopes) = recurse(header.items.into_iter(), &mut size_map);
    let top = Scope {
        name: "top".to_string(),
        scopes,
        variables,
    };

    (top, size_map)
}

struct VcdConverter {
    scope: Scope,
    /// Femtoseconds per timestep
    timescale: u128,

    alloc: MmappableAllocator,
    variables: HashMap<u64, KnownUnsizedVec<ValueChangeNode<Qit>, MmappableAllocator>, ahash::RandomState>,
}

impl VcdConverter {
    fn load_vcd<R: Read>(reader: R) -> io::Result<Self> {
        let mut parser = Parser::new(reader);
        let header = parser.parse_header()?;

        let timescale = if let Some((timesteps, unit)) = header.timescale {
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

        let (scope, mut size_map) = traverse_scopes(header);

        // let var_tree = find_all_scopes_and_variables(header);

        // let mut var_metas = HashMap::with_capacity_and_hasher(
        //     var_tree.variables.len(),
        //     ahash::RandomState::default(),
        // );

        // for (&var_id, var_info) in &var_tree.variables {
        //     var_metas.insert(
        //         var_id,
        //         VarMeta {
        //             qits: var_info.qits,
        //             last_value_change_offset: 0,
        //             number_of_value_changes: 0,
        //             last_timestamp_offset: 0,
        //             last_timestamp: 0,
        //         },
        //     );
        // }
        let alloc = MmappableAllocator::new();
        let mut variables = HashMap::with_hasher(ahash::RandomState::default());
        
        let start = Instant::now();
        let mut current_timestamp = 0;
        let mut processed_command_count = 0;

        #[cold]
        fn create_known_unsized_vec(size_map: &mut HashMap<VariableId, usize>, alloc: &MmappableAllocator, number: u64) -> KnownUnsizedVec<ValueChangeNode<Qit>, MmappableAllocator> {
            let size = size_map[&VariableId(number)];
            KnownUnsizedVec::with_capacity_in(size, 1, alloc.clone()) 
        }

        while let Some(command) = parser.next_command() {
            let command = command?;
            match command {
                Command::Timestamp(timestamp) => {
                    current_timestamp = timestamp;
                    processed_command_count += 1;
                }
                Command::ChangeVector(code, values) => {
                    let number = code.number();
                    let v = variables
                        .entry(number)
                        .or_insert_with(|| {
                            create_known_unsized_vec(&mut size_map, &alloc, number)
                        });
                    
                    v.push_from_iter(values.into_iter().copied().map(|value| match value {
                        vcd::Value::V0 => Qit::Zero,
                        vcd::Value::V1 => Qit::One,
                        vcd::Value::X => Qit::X,
                        vcd::Value::Z => Qit::Z,
                    }), current_timestamp);

                    processed_command_count += 1;
                }
                Command::ChangeScalar(code, value) => {
                    let number = code.number();
                    let v = variables
                        .entry(number)
                        .or_insert_with(|| {
                            create_known_unsized_vec(&mut size_map, &alloc, number)
                        });

                    v.push_from_iter(iter::once(match value {
                        vcd::Value::V0 => Qit::Zero,
                        vcd::Value::V1 => Qit::One,
                        vcd::Value::X => Qit::X,
                        vcd::Value::Z => Qit::Z,
                    }), current_timestamp);

                    processed_command_count += 1;
                },
                _ => {},
            }
        }

        let elapsed = start.elapsed();
        println!(
            "processed {} commands in {:.2} seconds",
            processed_command_count,
            elapsed.as_secs_f32()
        );

        Ok(Self {
            scope,
            timescale,

            alloc,
            variables,
        })
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

        println!("rough total usage: {} bytes", converter.alloc.rough_total_usage());

        // let db = converter.into_db()?;

        Err(anyhow!("not yet implemented"))
    }

    fn load_stream(
        &self,
        reader: &mut dyn Read,
    ) -> anyhow::Result<Box<dyn WaveformDatabase>> {
        let converter = VcdConverter::load_vcd(BufReader::with_capacity(1_000_000, reader))?;

        // println!("contains {} variables", converter.var_tree.variables.len());
        // let (&example_var_id, var_info) = converter.var_tree.variables.iter().nth(4).unwrap();
        // let mut reverse_value_changes = converter.iter_reverse_value_change(example_var_id);
        // println!(
        //     "variable \"{}\" ({}) has {} value changes",
        //     var_info.name,
        //     example_var_id,
        //     reverse_value_changes.len()
        // );
        // println!(
        //     "last value change: {:?}",
        //     reverse_value_changes.next().unwrap()
        // );
        // println!(
        //     "second to last value change: {:?}",
        //     reverse_value_changes.next().unwrap()
        // );
        // println!(
        //     "third to last value change: {:?}",
        //     reverse_value_changes.next().unwrap()
        // );

        Err(anyhow!("not yet implemented"))
    }
}
