#![no_main]

use libfuzzer_sys::fuzz_target;
use fst::varint::{encode_varint, decode_varint};

// Ugh this only works on Unix.

fuzz_target!(|data: u64| {
    let mut output: Vec<u8> = vec![0; 10];
    encode_varint(&mut output, value);
    assert_eq!(decode_varint(&output), Some(value));
});
