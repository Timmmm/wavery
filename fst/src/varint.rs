// use prusti_contracts::*;

use std::{io, slice};

/// Decode an unsigned varint. Return None if there was an error. This can
/// be because a) it overflows a u64, or b) we reach the end of the input.
pub fn decode_varint(input: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    let mut shift = 0;
    for byte in input {
        // Check for overflow.
        // This allows the compiler to unroll the loop. I'm not sure it is
        // faster tbh.
        if shift >= 64 {
            return None;
        }
        // Note that we don't check for overflow in the 10th byte (of which
        // only one bit is used), but never mind.
        value |= ((byte & 0x7F) as u64) << shift;
        // Check if we're finished.
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

/// Decode an signed varint. Return None if there was an error. This can
/// be because a) it overflows an i64, or b) we reach the end of the input.
pub fn decode_svarint(input: &[u8]) -> Option<i64> {
    let mut value: u64 = 0;
    let mut shift = 0;
    for byte in input {
        // Check for overflow.
        // This allows the compiler to unroll the loop. I'm not sure it is
        // faster tbh.
        if shift >= 64 {
            return None;
        }
        // Note that we don't check for overflow in the 10th byte (of which
        // only one bit is used), but never mind.
        value |= ((byte & 0x7F) as u64) << shift;
        // Check if we're finished.
        if byte & 0x80 == 0 {
            // Sign-extend if the top byte of `byte` is 1.
            if byte & 0x40 != 0 {
                value |= u64::MAX << (shift + 7);
            }
            return Some(value as i64);
        }
        shift += 7;
    }
    None
}

// The encoding functions are not used yet. I just added them to try out
// formal verification of the decode functions.

/// Encode an unsigned varint. Return the number of bytes written. There must be
/// enough space in the output. The maximum number of bytes written is 10.
pub fn encode_varint(output: &mut [u8], mut value: u64) -> usize {
    static MAX_BYTES: usize = 10; // 10 bytes with 7 bits each required for 64-bit.
    for i in 0..MAX_BYTES {
        let mut bits = value as u8 & 0x7F;
        value >>= 7;
        let more = value != 0;
        if more {
            bits |= 0x80;
        }
        output[i] = bits;
        if !more {
            return i + 1;
        }
    }
    MAX_BYTES
}

/// Encode an signed varint. Return the number of bytes written. There must be
/// enough space in the output. The maximum number of bytes written is 10.
pub fn encode_svarint(output: &mut [u8], mut value: i64) -> usize {
    static MAX_BYTES: usize = 10; // 10 bytes with 7 bits each required for 64-bit.
    for i in 0..MAX_BYTES {
        let mut bits = value as u8 & 0x7F;
        value >>= 7;
        // More if:
        // * the value has more non-sign bits in it, or
        // * the top bit of the current byte doesn't equal the sign bit.
        let more = (value != 0 && value != -1) || ((value as u8) & 0x40) != (bits & 0x40);
        if more {
            bits |= 0x80;
        }
        output[i] = bits;
        if !more {
            return i + 1;
        }
    }
    MAX_BYTES
}

// I decided to try various advanced test methods for the above. Unfortunately
// none of them work well on Windows.
//
// * cargo fuzz does not work on Windows at all.
// * Kani does not work on Windows at all either.
// * Cruesot possibly works on Windows, and I managed to compile it, but it relies
//   on Why3 which is written in OCaml. It turns out OCaml does not support
//   Windows very well. The instructions in the Cruesot readme tell you to
//   use "opam" to install things. The Opam docs direct you to a page to install
//   it for Windows but that page says it is deprecated and points you to some
//   other build tools and frankly I don't want to learn a whole fragmented
//   ecosystem just to use one program.
// * I had most success with Prusti, in that I actually got it to run. Unfortunately
//   it seems like a *lot* of Rust code is not supported. I got stuck at the error
//   "Access to reference type fields is not supported". It also doesn't support
//   iterators, and it doesn't support loops at all in pure functions.
//
// So I gave up.

// #[cfg(kani)]
// #[kani::proof]
// fn check_round_trip_kani() {
//     let value: u64 = kani::any();
//     let mut output: Vec<u8> = vec![0; 10];
//     encode_varint(&mut output, value);
//     assert_eq!(decode_varint(output), Some(value));
// }

pub trait VarintReader {
    fn read_varint(&mut self) -> io::Result<u64>;
    fn read_svarint(&mut self) -> io::Result<i64>;
}

impl<R> VarintReader for R
where
    R: io::Read,
{
    fn read_varint(&mut self) -> io::Result<u64> {
        let mut value: u64 = 0;
        let mut shift = 0;
        loop {
            let mut byte = 0;
            self.read_exact(slice::from_mut(&mut byte))?;

            // Check for overflow.
            // This allows the compiler to unroll the loop. I'm not sure it is
            // faster tbh.
            if shift >= 64 {
                return Err(io::Error::new(io::ErrorKind::Other, "varint overflow"));
            }
            // Note that we don't check for overflow in the 10th byte (of which
            // only one bit is used), but never mind.
            value |= ((byte & 0x7F) as u64) << shift;
            // Check if we're finished.
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn read_svarint(&mut self) -> io::Result<i64> {
        let mut value: u64 = 0;
        let mut shift = 0;
        loop {
            let mut byte = 0;
            self.read_exact(slice::from_mut(&mut byte))?;

            // Check for overflow.
            // This allows the compiler to unroll the loop. I'm not sure it is
            // faster tbh.
            if shift >= 64 {
                return Err(io::Error::new(io::ErrorKind::Other, "svarint overflow"));
            }
            // Note that we don't check for overflow in the 10th byte (of which
            // only one bit is used), but never mind.
            value |= ((byte & 0x7F) as u64) << shift;
            // Check if we're finished.
            if byte & 0x80 == 0 {
                // Sign-extend if the top byte of `byte` is 1.
                if byte & 0x40 != 0 {
                    value |= u64::MAX << (shift + 7);
                }
                return Ok(value as i64);
            }
            shift += 7;
        }
    }
}

/// Function to get the encoded lengths of a varint in bytes. I verified in Godbolt
/// that this generates pretty good unrolled assembly.
pub fn varint_length(mut value: u64) -> u8 {
    for x in 1..=10 {
        value >>= 7;
        if value == 0 {
            return x;
        }
    }
    unreachable!()
}

/// Function to get the encoded lengths of a varint in bytes. I verified in Godbolt
/// that this generates pretty good unrolled assembly.
// pub fn svarint_length(mut value: i64) -> u8 {
//     todo!()
// }

#[cfg(test)]
mod test {
    use super::*;

    fn check_round_trip_varint(value: u64) {
        let mut output: Vec<u8> = vec![0; 10];
        encode_varint(&mut output, value);
        assert_eq!(decode_varint(&output), Some(value));
    }

    #[test]
    fn test_round_trip_varint() {
        for value in 0..0xFFFF {
            check_round_trip_varint(value);
        }
        for value in (0xFFFF..0xFFFFFF).step_by(9) {
            check_round_trip_varint(value);
        }
        for value in (0xFFFFFF..0xFFFFFFFF).step_by(2135734) {
            check_round_trip_varint(value);
        }
        for value in 0xFFFF0000..=0xFFFFFFFF {
            check_round_trip_varint(value);
        }
    }

    fn check_round_trip_svarint(value: i64) {
        let mut output: Vec<u8> = vec![0; 10];
        encode_svarint(&mut output, value);
        assert_eq!(decode_svarint(&output), Some(value));
    }

    #[test]
    fn test_round_trip_svarint() {
        for value in 0..0xFFFF {
            check_round_trip_svarint(value);
        }
        for value in (0xFFFF..0xFFFFFF).step_by(9) {
            check_round_trip_svarint(value);
        }
        for value in -0xFFFF..0 {
            check_round_trip_svarint(value);
        }
        for value in (-0xFFFFFF..0xFFFF).step_by(9) {
            check_round_trip_svarint(value);
        }
    }

    /// Manually calculated examples (see the figures in the specification).
    #[test]
    fn test_manual_examples() {
        let mut output: Vec<u8> = vec![0; 10];
        assert_eq!(encode_varint(&mut output, 3141), 2);
        assert_eq!(output, [0xC5, 0x18, 0, 0, 0, 0, 0, 0, 0, 0]);

        let mut output: Vec<u8> = vec![0; 10];
        assert_eq!(encode_svarint(&mut output, -15429), 3);
        assert_eq!(output, [0xBB, 0x87, 0x7F, 0, 0, 0, 0, 0, 0, 0]);
    }
}
