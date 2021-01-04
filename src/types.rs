use std::fmt::{Debug, Display, Formatter};

pub trait SizeInBytes {
    fn size_in_bytes(count: usize) -> usize;
}

impl SizeInBytes for &'_ [u8] {
    fn size_in_bytes(count: usize) -> usize {
        count
    }
}

macro_rules! define_item_containers {
    ($slice_name:ident, $iter:ident, $vec_name:ident, $mask:literal, $item:ty, $bits_per_item:literal, $format_prefix:literal, [$(($item_variant:path, $bits:literal, $display:literal)),*]) => {
        #[derive(Copy, Clone, PartialEq, Eq)]
        pub struct $slice_name<'a> {
            size: usize,
            data: &'a [u8],
        }

        impl<'a> $slice_name<'a> {
            pub fn new(size: usize, data: &'a [u8]) -> Self {
                Self {
                    size,
                    data,
                }
            }
        }

        impl SizeInBytes for $slice_name<'_> {
            fn size_in_bytes(count: usize) -> usize {
                (count + (8 / $bits_per_item) - 1) / (8 / $bits_per_item)
            }
        }

        impl<'a> AsRef<[u8]> for $slice_name<'a> {
            fn as_ref(&self) -> &[u8] {
                self.data
            }
        }

        impl<'a> IntoIterator for $slice_name<'a> {
            type Item = $item;
            type IntoIter = $iter<'a>;

            fn into_iter(self) -> Self::IntoIter {
                $iter {
                    size: self.size,
                    index: 0,
                    data: self.data,
                }
            }
        }

        pub struct $iter<'a> {
            size: usize,
            index: usize,
            data: &'a [u8],
        }

        impl<'a> Iterator for $iter<'a> {
            type Item = $item;
            fn next(&mut self) -> Option<Self::Item> {
                if self.index < self.size {
                    let byte = self.data[(self.index as usize) / 8];
                    let in_index = (self.index * $bits_per_item) % 8;
                    let bit = (byte & ($mask << in_index)) >> in_index;
                    self.index += 1;

                    Some(match bit {
                        $(
                            $bits => $item_variant,
                        )*
                        _ => unimplemented!()
                    })
                } else {
                    None
                }
            }
        }

        impl Display for $slice_name<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                for bit in *self {
                    match bit {
                        $(
                            $item_variant => write!(f, $display)?,
                        )*
                    }
                }

                Ok(())
            }
        }

        impl Debug for $slice_name<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!("0", $format_prefix, "{}"), self)
            }
        }

        #[derive(Clone, PartialEq, Eq)]
        pub struct $vec_name {
            size: usize,
            data: Box<[u8]>,
        }

        impl $vec_name {
            pub fn new(size: usize, data: impl Into<Box<[u8]>>) -> Self {
                Self {
                    size,
                    data: data.into(),
                }
            }

            pub fn as_slice(&self) -> $slice_name {
                $slice_name {
                    size: self.size,
                    data: &*self.data,
                }
            }
        }

        impl SizeInBytes for $vec_name {
            fn size_in_bytes(count: usize) -> usize {
                (count + (8 / $bits_per_item) - 1) / (8 / $bits_per_item)
            }
        }

        impl Display for $vec_name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                for bit in self.as_slice() {
                    match bit {
                        $(
                            $item_variant => write!(f, $display)?,
                        )*
                    }
                }

                Ok(())
            }
        }

        impl Debug for $vec_name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!("0", $format_prefix, "{}"), self)
            }
        }
    };
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bit {
    Zero = 0,
    One = 1,
}

define_item_containers!(
    BitSlice,
    BitIter,
    BitVec,
    0b1,
    Bit,
    1,
    "b",
    [(Bit::Zero, 0, "0"), (Bit::One, 1, "1")]
);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Qit {
    Zero = 0,
    One = 1,
    X = 2,
    Z = 3,
}

impl Qit {
    pub fn bits_to_bytes(bits: usize) -> usize {
        (bits + 4 - 1) / 4
    }
}

define_item_containers!(
    QitSlice,
    QitIter,
    QitVec,
    0b11,
    Qit,
    2,
    "q",
    [
        (Qit::Zero, 0, "0"),
        (Qit::One, 1, "1"),
        (Qit::X, 2, "x"),
        (Qit::Z, 3, "z")
    ]
);
