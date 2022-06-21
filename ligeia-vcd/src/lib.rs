use std::{cell::Cell, io::Read, slice};

use fnv::FnvHashMap;
use ligeia_core::{
    meta::{self, ScopeId, StorageId},
    Ingestor,
};
use vcd::{Command, Header, IdCode, Parser, ScopeItem, Value, VarType};

pub fn load_vcd<R>(reader: R) -> Result<ligeia_core::Processed, Box<dyn std::error::Error>>
where
    R: Read,
{
    let mut parser = Parser::new(reader);
    let header = parser.parse_header()?;

    let femtoseconds_per_timestep = if let Some((timesteps, unit)) = header.timescale {
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

    let mut ingestor = Ingestor::new(femtoseconds_per_timestep)?;

    let storage_map = generate_scopes(&header, &mut ingestor);
    let mut buffer = vec![];

    loop {
        if let Some(command) = parser.next_command() {
            let command = command?;
            match command {
                Command::Timestamp(timestamp) => {
                    ingestor.ingest_timestep(meta::Timesteps(timestamp));
                }
                Command::ChangeVector(code, values) => {
                    let bytes = values.chunks(4).map(|chunk| {
                        let mut b = 0u8;
                        for (i, val) in chunk.iter().enumerate() {
                            let val = match val {
                                Value::V0 => 0,
                                Value::V1 => 1,
                                Value::X => 2,
                                Value::Z => 3,
                            };
                            b |= val << (i * 2);
                        }
                        b
                    });
                    buffer.clear();
                    buffer.extend(bytes);

                    ingestor.ingest_value(ligeia_core::Value {
                        storage_id: storage_map[&code],
                        data: &buffer,
                    })?;
                }
                Command::ChangeScalar(code, value) => {
                    ingestor.ingest_value(ligeia_core::Value {
                        storage_id: storage_map[&code],
                        data: slice::from_ref(&match value {
                            Value::V0 => 0,
                            Value::V1 => 1,
                            Value::X => 2,
                            Value::Z => 3,
                        }),
                    })?;
                }
                _ => {}
            }
        } else {
            break;
        }
    }

    Ok(ingestor.finish()?)
}

fn generate_scopes(header: &Header, ingestor: &mut Ingestor) -> FnvHashMap<IdCode, StorageId> {
    fn recurse<F1, F2>(
        ingestor: &mut Ingestor,
        items: &[ScopeItem],
        parent: meta::ScopeId,
        storage_map: &mut FnvHashMap<IdCode, StorageId>,
        scope_gen: &F1,
        storage_gen: &F2,
    ) where
        F1: Fn() -> ScopeId,
        F2: Fn() -> StorageId,
    {
        for item in items {
            match item {
                ScopeItem::Scope(scope) => {
                    let id = scope_gen();
                    ingestor.ingest_scope(meta::Scope {
                        id,
                        parent,
                        name: scope.identifier.clone(),
                    });

                    recurse(
                        ingestor,
                        &scope.children,
                        id,
                        storage_map,
                        scope_gen,
                        storage_gen,
                    );
                }
                ScopeItem::Var(var) => {
                    let storage_id = storage_gen();
                    storage_map.insert(var.code, storage_id);

                    let kind = match var.var_type {
                        VarType::Wire => meta::VarKind::Integer {
                            storages: vec![storage_id],
                            msb_index: var.size - 1,
                            lsb_index: 0,
                            signedness: meta::Signedness::Unsigned,
                        },
                        VarType::String => meta::VarKind::Utf8 {
                            storage: storage_id,
                        },
                        _ => unimplemented!(
                            "only wires and strings are supported in the VCD parser for the moment"
                        ),
                    };

                    ingestor.ingest_storage(meta::Storage {
                        id: storage_id,
                        ty: meta::StorageType::FourLogic,
                        start: 0,
                        width: var.size,
                    });

                    ingestor.ingest_var(meta::Var {
                        kind,
                        name: var.reference.clone(),
                        scope_id: parent,
                    });
                }
            }
        }
    }

    let scope_counter = Cell::new(1);
    let storage_counter = Cell::new(0);
    let scope_gen = || ScopeId(scope_counter.replace(scope_counter.get() + 1));
    let storage_gen = || StorageId(storage_counter.replace(storage_counter.get() + 1));

    let mut storage_map = FnvHashMap::default();

    recurse(
        ingestor,
        &header.items,
        ScopeId::ROOT,
        &mut storage_map,
        &scope_gen,
        &storage_gen,
    );

    storage_map
}
