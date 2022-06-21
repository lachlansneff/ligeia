use std::ops::{Add, AddAssign};

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct StorageId(pub u32);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ScopeId(pub u32);

impl ScopeId {
    pub const ROOT: ScopeId = ScopeId(0);
}

/// Some number of timesteps.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Timesteps(pub u64);

impl Add for Timesteps {
    type Output = Timesteps;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Timesteps {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

#[derive(Debug)]
pub struct Scope {
    pub name: String,
    pub id: ScopeId,
    pub parent: ScopeId,
}

#[derive(Debug, Clone, Copy)]
pub enum StorageType {
    TwoLogic,
    FourLogic,
    NineLogic,
}

#[derive(Debug)]
pub struct Storage {
    pub id: StorageId,
    pub ty: StorageType,
    pub width: u32,
    pub start: u32,
}

#[derive(Debug)]
pub struct EnumValue {
    pub name: String,
    pub value: Vec<bool>,
}

#[derive(Debug, Clone, Copy)]
pub enum Signedness {
    SignedTwosComplement,
    Unsigned,
}

#[derive(Debug)]
pub enum VarKind {
    None,
    Integer {
        storages: Vec<StorageId>,
        msb_index: u32,
        lsb_index: u32,
        signedness: Signedness,
    },
    Enum {
        storage: StorageId,
        values: Vec<EnumValue>,
    },
    Utf8 {
        storage: StorageId,
    },
}

#[derive(Debug)]
pub struct Var {
    pub name: String,
    pub scope_id: ScopeId,
    pub kind: VarKind,
}
