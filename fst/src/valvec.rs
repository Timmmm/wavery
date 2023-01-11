use std::fmt::Write;

// use crate::fst::VarLength;

// use anyhow::{bail, Result};
// use byteorder::{LittleEndian, ReadBytesExt};

/// Storage for an array of wave values. The type of all the values must be
/// the same but that type is type erased.
///
/// Each value can be 0-2^32-1 bits. The values of the bits can be 0, 1, X or Z,
/// or a load of other values used by VHDL (9 in total). However in most
/// cases they'll be mostly 0, 1, or X. Some simulators (e.g. Verilator) don't
/// support X so in that case they'll always be 0 or 1. So we need a way to
/// efficiently encode that. I think I'll do it roughly similarly to how
/// they are encoded in FST. There will be 1 extra bit to indicate "this value
/// contains something other than 0 and 1". If that is set then you look up
/// the value in an FxHashMap. The downside of that is that 8, 16 and 32 bit
/// signals are probably quite common and then they'll overflow into 2, 3 or 5 bytes.
///
/// Maybe I just concatenate all the bits and use bit shifting?
///
/// Ah ok, so the wave data will be stored in a huge Vec<> of concatenated compressed
/// values (better compressed than in FST). We will also store a block index
/// that points to every Nth value so you can easily decode less than the whole
/// thing if you want. Each block can use a different encoding, so we only
/// have to use inefficient encodings where X, Z etc. are actually present.
///
/// Within the block the encoding will be as follows depending on the length and
/// values:
///
/// | Values  | Bits | Encoding              |
/// |---------|------|-----------------------|
/// | 0/1     | 1    | Packed 8 per byte     |
/// | 0/1     | 2    | Packed 4 per byte     |
/// | 0/1     | 3    | Packed 2 per byte     |
/// | 0/1     | 4    | Packed 2 per byte     |
/// | 0/1     | 5    | Packed 1 per byte     |
/// | 0/1     | 6    | Packed 1 per byte     |
/// | 0/1     | 7    | Packed 1 per byte     |
/// | 0/1     | >=8  | Not packed            |
/// | 0/1/X/Z | 1    | Packed 4 per byte     |
/// | 0/1/X/Z | 2    | Packed 2 per byte     |
/// | 0/1/X/Z | >=3  | Not packed            |
/// | 0/1/X/Z/?/... | Any  | 2 bits per byte   |
///
/// When we also need to encode times, we encode the start time for each block
/// fully, and then a varint for the time delta for each value. We also encode
/// a base shift, so if all the times are like 100000, 200000, 300000, we encode
/// shift=5; 1, 2, 3  (but in binary).

// Very simple for now. TODO: Fancy scheme above.
pub type ValVec = Vec<Value>;
pub type ValAndTimeVec = Vec<(u64, Value)>;

// With 16 bytes this is the same size as Vec<> (24 bytes). Any more and it is
// bigger. This allows storing 64 bits on the stack.
#[derive(Eq, PartialEq, Clone, Debug, Default)]
pub struct Value(pub tinyvec::TinyVec<[u8; 16]>);

// pub struct ValVec {
//     /// Data that encodes the data.
//     data: Vec<u8>,
//     /// Offset into data of every Nth value.
//     block_offsets: Vec<usize>,
//     /// How many values stored in each block.
//     block_len: usize,
//     /// Number of bits the value is.
//     var_length: VarLength,
// }

// impl ValVec {
//     pub fn value(index: usize) -> u64 {
//         todo!()
//     }
// }

// pub struct ValAndTimeVec {
//     /// Data that encodes the data.
//     data: Vec<u8>,
//     /// Offset into data of every Nth value.
//     block_offsets: Vec<usize>,
//     /// How many values stored in each block.
//     block_len: usize,
//     /// Number of bits the value is.
//     var_length: VarLength,
// }

// impl ValAndTimeVec {
//     pub fn time_and_value(index: usize) -> (u64, u64) {
//         todo!()
//     }
// }
