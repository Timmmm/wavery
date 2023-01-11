//! Gtkwave FST format support
//!
//! # Format Specification
//!
//! I reverse engineered a specification from the GtkWave source code here:
//! https://blog.timhutt.co.uk/fst_spec/

pub mod fst;
pub mod valvec;
pub mod varint;

// use anyhow::Result;
// use std::collections::HashSet;

// #[derive(From, Into)]
// struct VarId(usize);

// trait Waves {
//     /// Get the design hiearchy.
//     fn hierarchy(&self) -> &Hierarchy;

//     /// Ensure the given set of waves are loaded. Does not reload them if they
//     /// are already loaded.
//     fn load_waves(&mut self, varids: HashSet<usize>) -> Result<()>;

//     /// Get the values of a wave. Must already be loaded.
//     fn wave(&self, varid: VarId) -> Result<&Wave<u8>>; // TODO: How to handle different types?

//     /// Get the points in time at which the wave values change.
//     fn times(&self) -> &[u64];

//     /// Get the timebase order of magnitude.
//     fn timebase_order(&self) -> i8;

//     /// Get info about a variable.
//     fn variable_info(&self, varid: VarId) -> Result<&VariableInfo>;
// }

// struct Hierarchy {
//     design: Scope,
// }

// struct Scope {
//     name: String,
//     children: Vec<Scope>,
//     variables: Vec<VarId>,
// }

// struct VariableInfo {
//     name: String,
//     direction: (),
//     bits: u64,
//     type_: String,
// }

// struct Wave<T> {
//     changes: Vec<(u64, T)>,
// }
