//! Support for decoders. These can have 1 or more input channels and 1 or more
//! output channels. The channels can be waves or transactions.
//!
//! When a decoder is added, the input data is fed into it, then it does anything
//! it wants and produces the output. Examples might be an SPI or I2C decoder,
//! and instruction decoder etc.
//!
//! Decoders will be written in WASM using Extism.
