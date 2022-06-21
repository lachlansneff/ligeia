use fnv::FnvHashMap;
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    mem,
};
use tempfile::tempfile;

use crate::meta::{ScopeId, StorageId, Timesteps};

pub mod meta;

pub struct Value<'a> {
    pub storage_id: StorageId,
    /// This is expected to be in the format used within SVCB `VALUE_CHANGE` blocks.
    pub data: &'a [u8],
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("an i/o error occured")]
    Io(#[from] io::Error),
}

struct Block {
    bytes: u32,
    block_size: usize,
    data: Box<[u8]>,
    offset: usize,
    // (Block offset, block size)
    block_offsets: Vec<(u64, usize)>,
}

impl Block {
    pub fn new(bytes: u32) -> Self {
        let block_size = (10 * 1024).max(bytes as usize + mem::size_of::<Timesteps>());
        Self {
            bytes,
            block_size,
            data: vec![0; block_size].into_boxed_slice(),
            offset: 0,
            block_offsets: vec![],
        }
    }

    #[cold]
    pub fn flush<W>(&mut self, mut writer: W, writer_offset: &mut u64) -> Result<(), io::Error>
    where
        W: Write,
    {
        writer.write_all(&self.data[..self.offset])?;
        self.block_offsets.push((*writer_offset, self.offset));
        *writer_offset += self.offset as u64;
        self.offset = 0;

        Ok(())
    }

    pub fn push<W>(
        &mut self,
        writer: W,
        writer_offset: &mut u64,
        timestamp: Timesteps,
        data: &[u8],
    ) -> Result<(), io::Error>
    where
        W: Write,
    {
        if mem::size_of::<Timesteps>() + data.len() > self.offset + data.len() {
            self.flush(writer, writer_offset)?;
        }

        self.data[self.offset..][..mem::size_of::<Timesteps>()]
            .copy_from_slice(&timestamp.0.to_le_bytes());
        self.offset += mem::size_of::<Timesteps>();

        let (actual_data, remaining) =
            self.data[self.offset..][..self.bytes as usize].split_at_mut(data.len());
        actual_data.copy_from_slice(data);
        remaining.fill(0);
        self.offset += self.bytes as usize;

        Ok(())
    }

    pub fn commit<W>(
        mut self,
        writer: W,
        writer_offset: &mut u64,
    ) -> Result<CommittedBlocks, io::Error>
    where
        W: Write,
    {
        self.flush(writer, writer_offset)?;
        Ok(CommittedBlocks {
            bytes: self.bytes,
            block_size: self.block_size,
            block_offsets: self.block_offsets,
        })
    }
}

struct CommittedBlocks {
    bytes: u32,
    block_size: usize,
    block_offsets: Vec<(u64, usize)>,
}

impl CommittedBlocks {
    pub fn read_blocks<R, F>(&self, mut reader: R, mut f: F) -> Result<(), io::Error>
    where
        R: Read + Seek,
        F: FnMut(Timesteps, &[u8]),
    {
        let mut buffer = vec![0; self.block_size];

        for &(offset, block_size) in &self.block_offsets {
            reader.seek(SeekFrom::Start(offset))?;
            reader.read_exact(&mut buffer[..block_size])?;

            for sub_offset in
                (0..block_size).step_by(self.bytes as usize + mem::size_of::<Timesteps>())
            {
                let timestamp = Timesteps(u64::from_le_bytes(
                    buffer[sub_offset..sub_offset + mem::size_of::<Timesteps>()]
                        .try_into()
                        .unwrap(),
                ));
                let data = &buffer[sub_offset + mem::size_of::<Timesteps>()..];
                f(timestamp, data);
            }
        }

        Ok(())
    }
}

pub struct Ingestor {
    femtoseconds_per_timestep: u128,
    scopes: FnvHashMap<ScopeId, meta::Scope>,
    vars: Vec<meta::Var>,
    storages: FnvHashMap<StorageId, meta::Storage>,
    current_timestep: Timesteps,
    writer: BufWriter<File>,
    writer_offset: u64,
    blocks: FnvHashMap<StorageId, Block>,
}

impl Ingestor {
    pub fn new(femtoseconds_per_timestep: u128) -> Result<Self, Error> {
        let writer = BufWriter::new(tempfile()?);

        Ok(Self {
            femtoseconds_per_timestep,
            scopes: FnvHashMap::default(),
            vars: vec![],
            storages: FnvHashMap::default(),
            current_timestep: Timesteps(0),
            writer,
            writer_offset: 0,
            blocks: FnvHashMap::default(),
        })
    }

    pub fn ingest_scope(&mut self, scope: meta::Scope) {
        self.scopes.insert(scope.id, scope);
    }

    pub fn ingest_var(&mut self, var: meta::Var) {
        self.vars.push(var);
    }

    pub fn ingest_storage(&mut self, storage: meta::Storage) {
        assert_eq!(storage.start, 0, "for now, storage.start must be 0");

        let id = storage.id;
        let bytes = match storage.ty {
            meta::StorageType::TwoLogic => (storage.width + 7) / 8, // 8 bits per byte
            meta::StorageType::FourLogic => (storage.width + 3) / 4, // 4 qits per byte
            meta::StorageType::NineLogic => storage.width,          // 1 nit per byte
        };

        self.storages.insert(id, storage);
        self.blocks.insert(id, Block::new(bytes));
    }

    pub fn ingest_timestep(&mut self, new: Timesteps) {
        self.current_timestep = new;
    }

    pub fn ingest_value(&mut self, value: Value) -> Result<(), Error> {
        self.blocks.get_mut(&value.storage_id).unwrap().push(
            &mut self.writer,
            &mut self.writer_offset,
            self.current_timestep,
            value.data,
        )?;

        Ok(())
    }

    pub fn finish(self) -> Result<Processed, Error> {
        let mut writer = self.writer;
        let mut writer_offset = self.writer_offset;

        let blocks = self
            .blocks
            .into_iter()
            .map(|(id, block)| Ok((id, block.commit(&mut writer, &mut writer_offset)?)))
            .collect::<Result<_, io::Error>>()?;

        Ok(Processed {
            femtoseconds_per_timestep: self.femtoseconds_per_timestep,
            scopes: self.scopes,
            vars: self.vars,
            storages: self.storages,
            reader: BufReader::new(writer.into_inner().unwrap()),
            blocks,
        })
    }
}

pub struct Processed {
    femtoseconds_per_timestep: u128,
    scopes: FnvHashMap<ScopeId, meta::Scope>,
    vars: Vec<meta::Var>,
    storages: FnvHashMap<StorageId, meta::Storage>,

    reader: BufReader<File>,
    blocks: FnvHashMap<StorageId, CommittedBlocks>,
}

impl Processed {
    pub fn femtoseconds_per_timestep(&self) -> u128 {
        self.femtoseconds_per_timestep
    }

    /// Temporary for testing
    pub fn storage_ids(&self) -> Vec<StorageId> {
        self.storages.keys().copied().collect()
    }

    pub fn within_scope(&self, id: ScopeId) -> (Vec<&meta::Scope>, Vec<&meta::Var>) {
        let scopes = self.scopes.values().filter(|s| s.parent == id).collect();
        let vars = self.vars.iter().filter(|v| v.scope_id == id).collect();

        (scopes, vars)
    }

    pub fn load_storage<F>(&mut self, id: StorageId, f: F) -> Result<(), Error>
    where
        F: FnMut(Timesteps, &[u8]),
    {
        self.blocks[&id].read_blocks(&mut self.reader, f)?;
        Ok(())
    }
}
