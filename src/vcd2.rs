use std::{collections::HashMap, fmt::Display, fs::File, io::{self, BufReader, Read}, iter, time::{Duration, Instant}};
use lazy_format::lazy_format;
use vcd::{Command, Parser, ScopeItem};
use crate::{db::{Scope, Variable, VariableId, VariableInfo, WaveformLoader, Forest, NodeTree, TreeOrLayer, Waveform}, info_bars::{BarFormatter, InfoBar, InfoBars}, mmap_alloc::MmappableAllocator, unsized_types::{BitSlice, BitType, KnownUnsizedVec, Qit, ValueChangeNode}};

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

pub struct VcdLoader {}

impl VcdLoader {
    pub const fn new() -> Self {
        Self {}
    }

    fn load_vcd<R: Read>(reader: R, mut after_every_command: impl FnMut(&R)) -> io::Result<(Waveform, LoadingFinished)> {
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

        let alloc = MmappableAllocator::new();
        let mut variables = HashMap::with_hasher(ahash::RandomState::default());
        
        let start = Instant::now();
        let mut current_timestamp = 0;
        let mut processed_command_count = 0;

        #[cold]
        fn create_known_unsized_vec(size_map: &mut HashMap<VariableId, usize>, alloc: &MmappableAllocator, id: VariableId) -> KnownUnsizedVec<ValueChangeNode<Qit>, MmappableAllocator> {
            let size = size_map[&id];
            KnownUnsizedVec::with_capacity_in(size, 1, alloc.clone()) 
        }

        loop {
            if let Some(command) = parser.next_command() {
                let command = command?;
                match command {
                    Command::Timestamp(timestamp) => {
                        current_timestamp = timestamp;
                        processed_command_count += 1;
                    }
                    Command::ChangeVector(code, values) => {
                        let id = VariableId(code.number());
                        let v = variables
                            .entry(id)
                            .or_insert_with(|| {
                                create_known_unsized_vec(&mut size_map, &alloc, id)
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
                        let id = VariableId(code.number());
                        let v = variables
                            .entry(id)
                            .or_insert_with(|| {
                                create_known_unsized_vec(&mut size_map, &alloc, id)
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
            } else {
                break
            }

            after_every_command(parser.reader());
        }

        let layers = variables
            .into_iter()
            .map(|(id, value_change)| {
                (id, NodeTree::Qit(TreeOrLayer::Layer(value_change)))
            });

        let time_elapsed = start.elapsed();
        let waveform = Waveform {
            top: scope,
            timescale,

            forest: Forest::new(alloc, layers),
        };

        let loading_finished = LoadingFinished {
            time_elapsed,
            command_count: processed_command_count,  
        };

        Ok((waveform, loading_finished))
    }
}

fn progress_bar_render<'a>(terminal_width: u16, bar: &'a dyn Display, progress: usize, total: usize) -> Box<dyn Display + 'a> {
    use termion::{color, style};
    use yapb::prefix::Binary;
    Box::new(lazy_format!(
        "{bold}loading{reset}: [{blue}{:width$}{reset}] {bold}{memory_used}B{style_reset}/{bold}{memory_avail}B{style_reset} ({percent:2.0}%)",
        bar,
        blue=color::Fg(color::Blue),
        reset=color::Fg(color::Reset),
        style_reset=style::Reset,
        bold=style::Bold,
        width=(terminal_width as usize) - 35,

        memory_used=Binary(progress as f64),
        memory_avail=Binary(total as f64),

        percent=(progress as f32 / total as f32) * 100.0,
    ))
}

struct LoadingFinished {
    time_elapsed: Duration,
    command_count: usize,
}

impl BarFormatter for LoadingFinished {
    fn format<'a>(&self, _terminal_width: u16, _bar: &'a dyn Display, _progress: usize, total: usize) -> Box<dyn Display + 'a> {
        use termion::style;
        use yapb::prefix::Binary;

        let command_count = self.command_count;
        let time_elapsed = self.time_elapsed.as_secs_f32();

        Box::new(lazy_format!(
            "{bold}{size}B{style_reset} loaded successfully ({bold}{commands}{style_reset} commands in {bold}{time:.2}s{style_reset})",
            size=Binary(total as f64),
            bold=style::Bold,
            style_reset=style::Reset,
            commands=command_count,
            time=time_elapsed,
        ))
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
        info_bars: &InfoBars,
        path: &std::path::Path,
    ) -> anyhow::Result<Waveform> {
        let mut f = File::open(&path)?;
        let map = unsafe { mapr::Mmap::map(&f) };

        let waveform = match map {
            Ok(map) => {
                let progress_bar = info_bars.add(InfoBar::new(
                    map.len(),
                    progress_bar_render,
                ));
                let mut transient_command_count = 0;
                let total_len = map.len();
                let (waveform, loading_finished) = Self::load_vcd(&map[..], |map| {
                    transient_command_count += 1;
                    if transient_command_count == 10_000 {
                        progress_bar.set_progress(total_len - map.len());
                        transient_command_count = 0;
                    }
                })?;
                // info_bars.remove(progress_bar).unwrap();
                info_bars.replace(progress_bar, InfoBar::new(total_len, loading_finished)).unwrap();
                waveform
            },
            Err(_) => {
                println!("mmap failed, attempting to load file as a stream");

                // VcdConverter::load_vcd(BufReader::with_capacity(1_000_000, f))?
                return self.load_stream(info_bars, &mut f);
            }
        };

        Ok(waveform)
    }

    fn load_stream(
        &self,
        info_bars: &InfoBars,
        reader: &mut dyn Read,
    ) -> anyhow::Result<Waveform> {
        let (waveform, loading_finished) = Self::load_vcd(BufReader::with_capacity(1_000_000, reader), |_| {})?;
        
        info_bars.add(InfoBar::new(0, loading_finished));
        
        Ok(waveform)
    }
}
