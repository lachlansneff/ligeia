// use nom::{Err, IResult, bytes::streaming::take, combinator::map_res, error::{Error, ErrorKind}, number::streaming::le_u32};
use thiserror::Error;
use std::{convert::TryInto, num::NonZeroUsize, str};
use crate::types::{Bit, Qit, BitVec, BitSlice, QitSlice};

// type IResult<I, O> = Result<(I, O), Err<()>>;

#[derive(Error, Debug)]
pub enum Reason {
    #[error("storage id is not valid")]
    InvalidStorageId,
    #[error("bytes are not valid utf-8")]
    InvalidUTF8,
    #[error("an invalid signedness value was present")]
    InvalidSignedValue,
    #[error("an invalid variable interpretation value was present")]
    InvalidInterpretationValue,
    #[error("an invalid storage type was present")]
    InvalidStorageType,
    #[error("an invalid varint was present")]
    InvalidVarInt,
}

pub enum Error {
    Incomplete(Option<NonZeroUsize>),
    Failure(Reason),
}

type ParseResult<'i, T> = Result<(&'i [u8], T), Error>;

trait Parse<'i, Output = Self> {
    fn parse(i: &'i [u8]) -> ParseResult<'i, Output>;
}

trait ParseWith<'i, Extra, Output = Self> {
    fn parse_with(i: &'i [u8], extra: Extra) -> ParseResult<'i, Output>;
}

pub trait StorageLookup {
    fn lookup(&self, storage_id: u32) -> Option<&StorageDeclaration>;
}

fn take(count: usize) -> impl Fn(&[u8]) -> ParseResult<&[u8]> {
    // let count = count.into();
    move |i| {
        if i.len() < count {
            Err(Error::Incomplete(NonZeroUsize::new(count - i.len())))
        } else {
            let (taken, rest) = i.split_at(count);
            Ok((rest, taken))
        }
    }
}

impl<'i> Parse<'i> for u32 {
    fn parse(i: &[u8]) -> ParseResult<Self> {
        if let Ok(bytes) = i.try_into() {
            Ok((&i[4..], u32::from_le_bytes(bytes)))
        } else {
            Err(Error::Incomplete(NonZeroUsize::new(4 - i.len())))
        }
    }
}

pub struct Varu32;
pub struct Varu64;

impl<'i> Parse<'i, u32> for Varu32 {
    fn parse(i: &[u8]) -> ParseResult<u32> {
        let (x, size) = varint_simd::decode(i)
            .or_else(|e| match e {
                varint_simd::VarIntDecodeError::Overflow => Err(Error::Failure(Reason::InvalidVarInt)),
                varint_simd::VarIntDecodeError::NotEnoughBytes => Err(Error::Incomplete(None))
            })?;
        
        Ok((&i[size as usize..], x))
    }
}

impl<'i> Parse<'i, u64> for Varu64 {
    fn parse(i: &[u8]) -> ParseResult<u64> {
        let (x, size) = varint_simd::decode(i)
            .or_else(|e| match e {
                varint_simd::VarIntDecodeError::Overflow => Err(Error::Failure(Reason::InvalidVarInt)),
                varint_simd::VarIntDecodeError::NotEnoughBytes => Err(Error::Incomplete(None))
            })?;
        
        Ok((&i[size as usize..], x))
    }
}

impl<'i> Parse<'i> for String {
    fn parse(i: &[u8]) -> ParseResult<Self> {
        let (i, length) = u32::parse(i)?;

        let (i, bytes) = take(length as usize)(i)?;
        if let Ok(s) = str::from_utf8(bytes) {
            Ok((i, s.to_string()))
        } else {
            Err(Error::Failure(Reason::InvalidUTF8))
        }
    }
}

impl<'i, T: Parse<'i>> Parse<'i> for Vec<T> {
    fn parse(i: &'i [u8]) -> ParseResult<'i, Self> {
        let (mut input, length) = u32::parse(i)?;
        let mut v = Vec::with_capacity(length as usize);

        for _ in 0..length as usize {
            let (i, x) = T::parse(input)?;
            input = i;
            v.push(x);
        }
        Ok((input, v))
    }
}

impl<'i, E: Copy, T: ParseWith<'i, E>> ParseWith<'i, E> for Vec<T> {
    fn parse_with(i: &'i [u8], extra: E) -> ParseResult<'i, Self> {
        let (mut input, length) = u32::parse(i)?;
        let mut v = Vec::with_capacity(length as usize);

        for _ in 0..length as usize {
            let (i, x) = T::parse_with(input, extra)?;
            input = i;
            v.push(x);
        }
        Ok((input, v))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDeclaration {
    parent_scope_id: u32,
    scope_id: u32,
    name: String,
}

impl<'i> Parse<'i> for ScopeDeclaration {
    fn parse(i: &[u8]) -> ParseResult<Self> {
        let (i, parent_scope_id) = u32::parse(i)?;
        let (i, scope_id) = u32::parse(i)?;
        let (i, name) = String::parse(i)?;

        Ok((i, Self { parent_scope_id, scope_id, name }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signedness {
    SignedTwosComplement,
    Unsigned,
}

impl<'i> Parse<'i> for Signedness {
    fn parse(i: &[u8]) -> ParseResult<Self> {
        let (i, raw) = u32::parse(i)?;
        match raw {
            0 => Ok((i, Signedness::SignedTwosComplement)),
            1 => Ok((i, Signedness::Unsigned)),
            _ => Err(Error::Failure(Reason::InvalidSignedValue))
        }
    }
}

impl<'i> ParseWith<'i, usize> for BitVec {
    fn parse_with(i: &[u8], bits: usize) -> ParseResult<Self> {
        let (i, data) = take(Bit::bits_to_bytes(bits))(i)?;
        Ok((i, BitVec::new(bits, data)))
    }
}

impl<'i> ParseWith<'i, usize> for BitSlice<'i> {
    fn parse_with(i: &'i [u8], bits: usize) -> ParseResult<'i, Self> {
        let (i, data) = take(Bit::bits_to_bytes(bits))(i)?;
        Ok((i, BitSlice::new(bits, data)))
    }
}

impl<'i> ParseWith<'i, usize> for QitSlice<'i> {
    fn parse_with(i: &'i [u8], bits: usize) -> ParseResult<'i, Self> {
        let (i, data) = take(Qit::bits_to_bytes(bits))(i)?;
        Ok((i, QitSlice::new(bits, data)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumField {
    name: String,
    value: BitVec,
}

impl<'i> ParseWith<'i, usize> for EnumField {
    fn parse_with(i: &[u8], bits: usize) -> ParseResult<Self> {
        let (i, name) = String::parse(i)?;
        let (i, value) = BitVec::parse_with(i, bits)?;
        Ok((i, EnumField { name, value }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableInterpretation {
    Integer {
        storage_ids: Vec<u32>,
        msb: u32,
        lsb: u32,
        signedness: Signedness,
    },
    Enum {
        storage_id: u32,
        fields: Vec<EnumField>,
    },
    Other {
        storage_id: u32,
    }
}

impl<E: StorageLookup> ParseWith<'_, E> for VariableInterpretation {
    fn parse_with(i: &[u8], storages: E) -> ParseResult<Self> {
        let (i, interpretation) = u32::parse(i)?;

        match interpretation {
            0 | 2 | 3 => {
                let (i, storage_id) = u32::parse(i)?;
                if interpretation == 2 {
                    let bits = storages
                        .lookup(storage_id)
                        .ok_or_else(|| Error::Failure(Reason::InvalidStorageId))?
                        .length;

                    let (i, fields) = Vec::<EnumField>::parse_with(i, bits as usize)?;

                    Ok((i, VariableInterpretation::Enum { storage_id, fields }))
                } else {
                    Ok((i, VariableInterpretation::Other { storage_id }))
                }
            }
            1 => {
                let (i, storage_ids) = Vec::<u32>::parse(i)?;
                let (i, msb) = u32::parse(i)?;
                let (i, lsb) = u32::parse(i)?;
                let (i, signedness) = Signedness::parse(i)?;

                Ok((i, VariableInterpretation::Integer {
                    storage_ids,
                    msb,
                    lsb,
                    signedness,
                }))
            }
            _ => Err(Error::Failure(Reason::InvalidInterpretationValue))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableDeclaration {
    scope_id: u32,
    name: String,
    interpretation: VariableInterpretation,
}

impl<E: StorageLookup> ParseWith<'_, E> for VariableDeclaration {
    fn parse_with(i: &[u8], storages: E) -> ParseResult<Self> {
        let (i, scope_id) = u32::parse(i)?;
        let (i, name) = String::parse(i)?;

        let (i, interpretation) = VariableInterpretation::parse_with(i, storages)?;

        Ok((i, VariableDeclaration { scope_id, name, interpretation }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageType {
    Binary {
        lsb: u32,
    },
    Quaternary {
        lsb: u32,
    },
    Utf8,
}

impl Parse<'_> for StorageType {
    fn parse(i: &[u8]) -> ParseResult<Self> {
        let (i, ty) = u32::parse(i)?;
        match ty {
            0 => {
                let (i, lsb) = u32::parse(i)?;
                Ok((i,
                    StorageType::Binary {
                        lsb,
                    }
                ))
            },
            1 => {
                let (i, lsb) = u32::parse(i)?;
                Ok((i,
                    StorageType::Quaternary {
                        lsb,
                    }
                ))
            },
            2 => {
                Ok((i, StorageType::Utf8))
            },
            _ => {
                Err(Error::Failure(Reason::InvalidStorageType))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDeclaration {
    id: u32,
    ty: StorageType,
    length: u32,
}

impl Parse<'_> for StorageDeclaration {
    fn parse(i: &[u8]) -> ParseResult<Self> {
        let (i, id) = u32::parse(i)?;
        let (i, ty) = StorageType::parse(i)?;
        let (i, length) = u32::parse(i)?;

        Ok((i, Self { id, ty, length }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestep(u64);

impl Parse<'_> for Timestep {
    fn parse(i: &[u8]) -> ParseResult<Self> {
        let (i, timestep) = Varu64::parse(i)?;
        Ok((i, Self(timestep)))
    }
}

pub enum ValueChange<'a> {
    Binary(BitSlice<'a>),
    Quaternary(QitSlice<'a>),
    Utf8(&'a [u8]),
}

impl<'i, L: StorageLookup> ParseWith<'i, &'_ L> for ValueChange<'i> {
    fn parse_with(i: &'i [u8], storages: &L) -> ParseResult<'i, Self> {
        let (i, storage_id) = Varu32::parse(i)?;

        let storage = storages
            .lookup(storage_id)
            .ok_or_else(|| Error::Failure(Reason::InvalidStorageId))?;
        
        match storage.ty {
            StorageType::Binary { lsb } => {
                let (i, bitslice) = BitSlice::parse_with(i, (storage.length - lsb) as usize)?;

                Ok((i, ValueChange::Binary(bitslice)))
            }
            StorageType::Quaternary { lsb } => {
                let (i, qitslice) = QitSlice::parse_with(i, (storage.length - lsb) as usize)?;

                Ok((i, ValueChange::Quaternary(qitslice)))
            }
            StorageType::Utf8 => {
                let (i, slice) = take(storage.length as usize)(i)?;

                Ok((i, ValueChange::Utf8(slice)))
            }
        }
    }
}

pub struct ValueChanges<'a, L> {
    storages: L,
    remaining: usize,
    data: &'a [u8],
}

impl<'i, L> ParseWith<'i, L> for ValueChanges<'i, L> {
    fn parse_with(i: &'i [u8], storages: L) -> ParseResult<'i, Self> {
        let (i, count) = Varu32::parse(i)?;

        Ok((i, Self { storages, remaining: count as usize, data: i }))
    }
}

impl<'a, L: StorageLookup> Iterator for ValueChanges<'a, L> {
    type Item = Result<ValueChange<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;

        let (i, value_change) = match ValueChange::parse_with(self.data, &self.storages) {
            Ok(x) => x,
            Err(e) => return Some(Err(e)),
        };
        self.data = i;

        Some(Ok(value_change))
    }
}

pub enum Block {
    Scope(Box<ScopeDeclaration>),
    Variable(Box<VariableDeclaration>),
    Storage(Box<StorageDeclaration>),

    Timestep(u64),
    ValueChanges {
        count: u32,

    }
}
