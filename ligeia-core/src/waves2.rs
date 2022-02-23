use crate::logic2;


#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageId(pub u32);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScopeId(pub u32);

/// Some number of timesteps.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timesteps(pub u64);

#[derive(Clone, Copy)]
pub enum StorageType {
    TwoLogic,
    FourLogic,
    NineLogic,
}

pub struct Storage {
    pub ty: StorageType,
    pub width: u32,
    pub start: u32,
}

pub struct EnumValue {
    pub name: String,
    pub value: Vec<logic2::Two>,
}

#[derive(Clone, Copy)]
pub enum Signedness {
    SignedTwosComplement,
    Unsigned,
}

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

pub struct Var {
    pub name: String,
    pub kind: VarKind,
}
