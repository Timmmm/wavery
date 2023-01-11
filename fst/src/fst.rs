use crate::{
    valvec::{ValAndTimeVec, ValVec, Value},
    varint::{decode_svarint, decode_varint, varint_length, VarintReader},
};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom},
    ops::Range,
    path::{Path, PathBuf},
};

use log::info;

use anyhow::{bail, Context, Result};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use derive_more::{From, Into};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use flate2::read::ZlibDecoder;
use tinyvec::tiny_vec;
use typed_index_collections::TiVec;

#[derive(From, Into, Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
pub struct BlockId(usize);

#[derive(From, Into, Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
pub struct VarId(pub usize);

#[derive(From, Into, Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ScopeId(pub usize);

#[allow(non_camel_case_types)]
#[derive(FromPrimitive, Copy, Clone, Debug, Eq, PartialEq, Hash)]
enum BlockType {
    FST_BL_HDR = 0,
    FST_BL_VCDATA = 1,
    FST_BL_BLACKOUT = 2,
    FST_BL_GEOM = 3,
    FST_BL_HIER = 4,
    FST_BL_VCDATA_DYN_ALIAS = 5,
    FST_BL_HIER_LZ4 = 6,
    FST_BL_HIER_LZ4DUO = 7,
    FST_BL_VCDATA_DYN_ALIAS2 = 8,
    FST_BL_ZWRAPPER = 254,
    FST_BL_SKIP = 255,
}

static REAL_ENDIANNESS_LITTLE: u64 = 0x4005BF0A8B145769;
static REAL_ENDIANNESS_BIG: u64 = 0x6957148B0ABF0540;

// TODO: Use enum
const FST_ST_GEN_ATTRBEGIN: u8 = 252;
const FST_ST_GEN_ATTREND: u8 = 253;
const FST_ST_VCD_SCOPE: u8 = 254;
const FST_ST_VCD_UPSCOPE: u8 = 255;

#[derive(Clone, Debug)]
pub struct Header {
    pub start_time: u64,
    pub end_time: u64,
    // Note this is actually f64 but we only use it to compare bit patterns
    // which is easier as u64.
    pub real_endianness: u64,
    pub writer_memory_use: u64,
    pub num_scopes: u64,
    pub num_hiearchy_vars: u64,
    pub num_vars: u64,
    pub num_vc_blocks: u64,
    pub timescale: i8,
    pub writer: [u8; 128],
    pub date: [u8; 26],
    pub reserved: [u8; 93],
    pub filetype: u8,
    pub timezero: i64,
}

fn array_to_string<const T: usize>(x: &[u8; T]) -> String {
    String::from_utf8_lossy(&x[0..x.iter().position(|b| *b == 0).unwrap_or(x.len())]).to_string()
}

impl Header {
    pub fn writer_string(&self) -> String {
        array_to_string(&self.writer)
    }
    pub fn date_string(&self) -> String {
        array_to_string(&self.date)
    }
}

#[derive(Debug)]
pub enum BlackoutType {
    DumpOn,
    DumpOff,
}

/// The metadata about the value change block including pointers to the locations
/// in the file of the actual data so we can get there again quickly.
#[derive(Clone, Debug)]
pub struct ValueChangeBlockInfo {
    pub start_time: u64,
    pub end_time: u64,
    pub memory_required: u64,
    pub bits_uncompressed_length: u64,
    pub bits_compressed_length: u64,
    pub bits_count: u64,
    /// Offset in the file of this data from the start of the whole file.
    pub bits_data_offset: u64,
    pub waves_count: u64,
    pub waves_packtype: u8,
    /// Offset in the file of this data from the start of the whole file.
    pub waves_data_offset: u64,
    /// Offset in the file of this data from the start of the whole file.
    pub position_data_offset: u64,
    pub position_length: u64,
    /// Offset in the file of this data from the start of the whole file.
    pub time_data_offset: u64,
    pub time_uncompressed_length: u64,
    pub time_compressed_length: u64,
    pub time_count: u64,
}

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum VarLength {
    Bits(u32),
    Real,
}

#[derive(Clone, Debug)]
pub struct VarLengths {
    /// Geometry block is fully read into memory. There are two sentinel values.
    /// VAR_LENGTH_REAL for reals and VAR_LENGTH_LONG for when it was too
    /// big to fit. In that case it is in the var_lengths_long array.
    pub lengths: TiVec<VarId, u8>,

    /// Var lengths for vars that have the value VAR_LENGTH_LONG.
    pub lengths_long: HashMap<VarId, u32>,
}

impl VarLengths {
    pub fn length(&self, varid: VarId) -> VarLength {
        match self.lengths[varid] {
            VAR_LENGTH_REAL => VarLength::Real,
            VAR_LENGTH_LONG => VarLength::Bits(self.lengths_long[&varid]),
            x => VarLength::Bits(x as u32),
        }
    }
}

#[derive(Debug)]
pub struct ValueChangeBlockData {
    /// The medata for the value change block.
    pub info: ValueChangeBlockInfo,
    /// The value change times (since the start of time).
    pub times: Vec<u64>,
}

#[derive(Default, Debug)]
pub struct VarData {
    /// Its initial value in each Value Change block.
    pub initial_values: ValVec,
    /// The offset and length of its wave data in each Value Change block.
    /// An empty slice means there are no changes.
    pub wave_slices: TiVec<BlockId, Range<u64>>,
}

#[derive(Debug)]
pub struct Fst {
    /// File path that this file was loaded from, for convenience.
    pub filename: PathBuf,

    /// Header block fully read into memory.
    pub header: Header,

    /// Hierarchy block is fully read into memory.
    pub hierarchy: espalier::Tree<ScopeId, HierarchyScope>,

    /// Length of each variable in bits.
    pub var_lengths: VarLengths,

    /// The metadata for each Value Change block, and the times of the value changes.
    pub value_change_blocks: TiVec<BlockId, ValueChangeBlockData>,

    /// For each var, the initial value and wave offset in all the blocks.
    pub var_data: TiVec<VarId, VarData>,

    /// Blackout block is fully read into memory. This is optional.
    pub blackouts: Vec<(BlackoutType, u64)>,

    /// The file reader; used when actually reading the waves.
    reader: BufReader<File>,
}

const VAR_LENGTH_REAL: u8 = 0xFE;
const VAR_LENGTH_LONG: u8 = 0xFF;

#[derive(Debug, Default)]
pub struct HierarchyScope {
    /// This does not come from the file - it is just an incremental ID
    /// starting from 0, assigned in depth-first order. Use for convenience.
    pub type_: u8,
    pub name: String,
    pub component: String,
    pub vars: Vec<HierarchyVar>,
    pub attrs: Vec<HierarchyAttr>,
}

#[derive(Debug, Default)]
pub struct HierarchyVar {
    pub type_: u8,
    pub direction: u8,
    pub name: String,
    pub length: u64,
    pub id: VarId,
    pub is_alias: bool,
}

#[derive(Debug, Default)]
pub struct HierarchyAttr {
    pub type_: u8,
    pub subtype: u8,
    pub name: String,
    pub arg: u64,
    pub arg_from_name: u64,
}

trait ReadArray {
    fn read_array<const T: usize>(&mut self) -> std::io::Result<[u8; T]>;

    fn read_vec(&mut self, length: usize) -> std::io::Result<Vec<u8>>;

    fn read_tinyvec<const N: usize>(
        &mut self,
        length: usize,
    ) -> std::io::Result<tinyvec::TinyVec<[u8; N]>>;
}

impl<R> ReadArray for R
where
    R: Read,
{
    fn read_array<const T: usize>(&mut self) -> std::io::Result<[u8; T]> {
        let mut buf = [0; T];
        self.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn read_vec(&mut self, length: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0; length];
        self.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn read_tinyvec<const N: usize>(
        &mut self,
        length: usize,
    ) -> std::io::Result<tinyvec::TinyVec<[u8; N]>> {
        let mut buf = tinyvec::TinyVec::<[u8; N]>::with_capacity(length);
        buf.resize(length, 0);
        self.read_exact(&mut buf)?;
        Ok(buf)
    }
}

trait ReadString {
    fn read_null_terminated_string(&mut self, max_size: u64) -> std::io::Result<String>;
}

impl<R> ReadString for R
where
    R: BufRead,
{
    fn read_null_terminated_string(&mut self, max_size: u64) -> std::io::Result<String> {
        let mut buf = Vec::new();
        self.take(max_size).read_until(0, &mut buf)?;
        // It includes the 0 byte.
        buf.pop();
        Ok(String::from_utf8_lossy(&buf).to_string())
    }
}

impl Fst {
    pub fn load(filename: &Path) -> Result<Self> {
        let f = File::open(filename)?;

        let mut reader = BufReader::new(f);

        let mut expected_block_types: HashSet<BlockType> = Default::default();
        expected_block_types.insert(BlockType::FST_BL_HDR);

        let mut header = None;
        let mut value_change_blocks = TiVec::new();
        let mut var_data = TiVec::new();

        let mut hierarchy = None;
        let mut blackouts = None;

        let mut var_lengths = None;

        // Read blocks.
        while let Ok(block_type) = reader.read_u8() {
            let block_type = match BlockType::from_u8(block_type) {
                Some(b) => b,
                None => {
                    bail!("Unknown block type {}", block_type);
                }
            };

            if !expected_block_types.contains(&block_type) {
                bail!(
                    "Unexpected block type {:?}; expected one of {:?}",
                    &block_type,
                    &expected_block_types
                );
            }

            let block_length_position = reader.stream_position()?;

            let block_length_including_length = reader.read_u64::<BigEndian>()?;
            let block_length = block_length_including_length
                .checked_sub(8)
                .context("Invalid block length (must be >= 8).")?;

            match block_type {
                BlockType::FST_BL_HDR => {
                    if block_length != 321 {
                        bail!("Invalid header block length {block_length} (should be 321)");
                    }

                    let h = Self::read_header(&mut reader)?;
                    // One byte is not much a magic number so we use `e` too.
                    if h.real_endianness != REAL_ENDIANNESS_LITTLE
                        && h.real_endianness != REAL_ENDIANNESS_BIG
                    {
                        bail!("Not an FST file: {:x?}", h.real_endianness);
                    }

                    // Reserve the number of blocks.
                    value_change_blocks.reserve(h.num_vc_blocks as usize);

                    var_data.resize_with(h.num_vars as usize, Default::default);

                    header = Some(h);

                    expected_block_types.remove(&BlockType::FST_BL_HDR);
                    expected_block_types.insert(BlockType::FST_BL_VCDATA);
                    expected_block_types.insert(BlockType::FST_BL_BLACKOUT);
                    expected_block_types.insert(BlockType::FST_BL_GEOM);
                    expected_block_types.insert(BlockType::FST_BL_HIER);
                    expected_block_types.insert(BlockType::FST_BL_VCDATA_DYN_ALIAS);
                    expected_block_types.insert(BlockType::FST_BL_HIER_LZ4);
                    expected_block_types.insert(BlockType::FST_BL_HIER_LZ4DUO);
                    expected_block_types.insert(BlockType::FST_BL_VCDATA_DYN_ALIAS2);
                }
                BlockType::FST_BL_VCDATA => {
                    bail!("This file uses an old format (FST_BL_VCDATA) which is not currently supported.");
                }
                BlockType::FST_BL_BLACKOUT => {
                    blackouts = Some(Self::read_blackout_block(&mut reader)?);
                    // There should only be one blackout block.
                    expected_block_types.remove(&BlockType::FST_BL_BLACKOUT);
                }
                BlockType::FST_BL_GEOM => {
                    var_lengths = Some(Self::read_geometry_block(&mut reader, block_length)?);
                    // There should only be one geometry block.
                    expected_block_types.remove(&BlockType::FST_BL_GEOM);
                }
                BlockType::FST_BL_VCDATA_DYN_ALIAS => {
                    bail!("This file uses an old format (FST_BL_VCDATA_DYN_ALIAS) which is not currently supported.");
                }
                BlockType::FST_BL_HIER
                | BlockType::FST_BL_HIER_LZ4
                | BlockType::FST_BL_HIER_LZ4DUO => {
                    let num_scopes_hint = header
                        .as_ref()
                        .expect("Internal logic error; header not read before hierarchy.")
                        .num_scopes as usize;
                    hierarchy = Some(Self::read_hierarchy(
                        &mut reader,
                        block_type,
                        block_length,
                        num_scopes_hint,
                    )?);

                    expected_block_types.remove(&BlockType::FST_BL_HIER);
                    expected_block_types.remove(&BlockType::FST_BL_HIER_LZ4);
                    expected_block_types.remove(&BlockType::FST_BL_HIER_LZ4DUO);
                }
                BlockType::FST_BL_VCDATA_DYN_ALIAS2 => {
                    let data = Self::read_value_change_block(
                        &mut reader,
                        block_length,
                        // `expected_block_types` ensures this should not happen.
                        header
                            .as_ref()
                            .expect("Header not read before Value Change block")
                            .num_vars,
                        &mut var_data,
                    )?;

                    value_change_blocks.push(data);
                }
                BlockType::FST_BL_ZWRAPPER => {
                    bail!("This file is a GZip compressed FST file (FST_BL_ZWRAPPER) which is not currently supported. You should just compressed it separately to get `.fst.gz`.");
                }
                BlockType::FST_BL_SKIP => {
                    bail!("File contains 'skip' block indicating it has not been finished writing. Reading partially complete files is not currently supported.");
                }
            }

            // Verify we are at the end of the block.
            let pos = reader.stream_position()?;
            if pos != block_length_position + block_length_including_length {
                bail!("Error after reading block {:?} Expected to be at position {} + {} = {}, but actually at {}.",
                    block_type,
                    block_length_position,
                    block_length_including_length,
                    block_length_position + block_length_including_length,
                    pos,
                );
            }
        }

        let header = match header {
            Some(h) => h,
            None => {
                bail!("Empty file");
            }
        };

        let hierarchy = match hierarchy {
            Some(h) => h,
            None => {
                bail!("Missing hierarchy block");
            }
        };

        let var_lengths = match var_lengths {
            Some(v) => v,
            None => {
                bail!("Missing geometry block");
            }
        };

        let blackouts = blackouts.unwrap_or_default();

        // Read the initial values (the bit array) of each block here. We have
        // to do it at the end because we need `var_lengths` (the geometry block).

        for vc in value_change_blocks.iter() {
            reader.seek(SeekFrom::Start(vc.info.bits_data_offset))?;
            Self::read_bits_array(
                &mut reader,
                vc.info.bits_compressed_length,
                vc.info.bits_uncompressed_length,
                vc.info.bits_count,
                &var_lengths,
                &mut var_data,
            )?;
        }

        Ok(Self {
            filename: filename.to_owned(),
            header,
            value_change_blocks,
            var_lengths,
            blackouts,
            hierarchy,
            var_data,
            reader,
        })
    }

    /// This takes a mutable reference to self because it reads from the file.
    pub fn read_wave(&mut self, varid: VarId) -> Result<ValAndTimeVec> {
        // 1. Loop through the blocks.
        // 2. Get the wave offset.
        // 3. Decode the values to Value

        info!("Reading waves for {:?}", varid);

        let mut wave = ValAndTimeVec::new();

        let var_data = self.var_data.get(varid).context("Invalid var ID")?;
        let var_length = self.var_lengths.length(varid);

        // Add the initial value. TODO: Should this error if there is no initial value?
        if let Some(first) = var_data.initial_values.first() {
            info!("Initial value: {:?}", first);
            wave.push((0, first.clone()));
        }

        for (block, wave_slice) in self
            .value_change_blocks
            .iter()
            .zip(var_data.wave_slices.iter())
        {
            info!("Reading Value Change Block...");

            if wave_slice.is_empty() {
                info!("No changes in this block.");
                continue;
            }

            // Offset of the wave data.
            let offset = block.info.waves_data_offset + wave_slice.start;

            info!(
                "Offset of wave data in file: {} + {} = {}",
                block.info.waves_data_offset, wave_slice.start, offset
            );

            self.reader.seek(SeekFrom::Start(offset))?;

            // Read vc_waves_length. This is the uncompressed length if compressed
            // or 0 if not compressed. We don't actually use this because we
            // decompress on the fly.
            let uncompressed_length_or_zero = self.reader.read_varint()?;

            // Compressed length.
            let compressed_length = (wave_slice.end - wave_slice.start) as usize
                - varint_length(uncompressed_length_or_zero) as usize;

            // We have to read all the data into memory in most cases.
            // This also makes it easier to know when we've read to the end
            // of the wave.
            let compressed_data = self.reader.read_vec(compressed_length)?;

            info!(
                "Uncompressed length (0=not compressed): {} Pack type: {}",
                uncompressed_length_or_zero, block.info.waves_packtype as char
            );

            // The pack type and waves_length determine the compression used.
            let uncompressed_data = match (
                uncompressed_length_or_zero as usize,
                block.info.waves_packtype,
            ) {
                (0, _) => compressed_data,
                (uncompressed_length, b'F') => {
                    // FastLZ. Have to read the data into memory in this case.
                    let mut uncompressed_data = vec![0; uncompressed_length];
                    let output = fastlz::decompress(&compressed_data, &mut uncompressed_data)
                        .ok()
                        .context("FastLZ decompression")?;
                    if output.len() != uncompressed_data.len() {
                        bail!("Couldn't uncompress wave data using FastLZ");
                    }
                    uncompressed_data
                }
                (uncompressed_length, b'4') => {
                    // LZ4
                    lz4_flex::block::decompress(&compressed_data, uncompressed_length)?
                }
                (uncompressed_length, _) => {
                    // ZLib
                    let mut uncompressed_data = Vec::with_capacity(uncompressed_length);
                    flate2::Decompress::new(false).decompress(
                        &compressed_data,
                        &mut uncompressed_data,
                        flate2::FlushDecompress::Finish,
                    )?;
                    uncompressed_data
                }
            };

            // Get the actual uncompressed length (it could have been zero).
            let uncompressed_length = uncompressed_data.len();

            let mut cursor = Cursor::new(uncompressed_data);

            let mut time_index = 0;

            while (cursor.position() as usize) < uncompressed_length {
                // info!("Reader pos: {}", cursor.position());
                let (value, time_index_delta) =
                    value_and_time_index_delta_from_waves_table(&mut cursor, var_length)?;
                // info!("Read value and time index delta: {:?}, {:?}", value, time_index_delta);
                time_index += time_index_delta;
                let time = block.times[time_index as usize];
                wave.push((time, value));
            }
        }

        Ok(wave)
    }

    fn read_header(reader: &mut impl BufRead) -> Result<Header> {
        Ok(Header {
            start_time: reader.read_u64::<BigEndian>()?,
            end_time: reader.read_u64::<BigEndian>()?,
            real_endianness: reader.read_u64::<LittleEndian>()?,
            writer_memory_use: reader.read_u64::<BigEndian>()?,
            num_scopes: reader.read_u64::<BigEndian>()?,
            num_hiearchy_vars: reader.read_u64::<BigEndian>()?,
            num_vars: reader.read_u64::<BigEndian>()?,
            num_vc_blocks: reader.read_u64::<BigEndian>()?,
            timescale: reader.read_i8()?,
            writer: reader.read_array()?,
            date: reader.read_array()?,
            reserved: reader.read_array()?,
            filetype: reader.read_u8()?,
            timezero: reader.read_i64::<BigEndian>()?,
        })
    }

    fn read_hierarchy(
        reader: &mut (impl BufRead + Seek),
        block_type: BlockType,
        block_length: u64,
        num_scopes_hint: usize,
    ) -> Result<espalier::Tree<ScopeId, HierarchyScope>> {
        let start_pos = reader.stream_position()?;

        let uncompressed_length = reader.read_u64::<BigEndian>()?;

        let uncompressed_data;
        let mut uncompressed_cursor;

        let mut compressed_reader: &mut dyn BufRead = match block_type {
            BlockType::FST_BL_HIER => reader,
            BlockType::FST_BL_HIER_LZ4 => {
                // Unfortunately the LZ4 compression is done with the block format, and
                // lz4_flex does not support streaming reads using that. I think that
                // theoretically it could, but it would need to take a BufRead.

                // For now just read into memory.
                let data = reader.read_vec(
                    block_length
                        .checked_sub(8)
                        .context("Invalid block length")? as usize,
                )?;

                uncompressed_data = lz4_flex::decompress(&data, uncompressed_length as usize)?;
                uncompressed_cursor = Cursor::new(uncompressed_data);
                &mut uncompressed_cursor
            }
            BlockType::FST_BL_HIER_LZ4DUO => {
                let compressed_once_length = reader.read_u64::<BigEndian>()?;

                let data = reader.read_vec(
                    block_length
                        .checked_sub(16)
                        .context("Invalid block length")? as usize,
                )?;

                let uncompressed_data_once =
                    lz4_flex::decompress(&data, compressed_once_length as usize)?;

                uncompressed_data =
                    lz4_flex::decompress(&uncompressed_data_once, uncompressed_length as usize)?;
                uncompressed_cursor = Cursor::new(uncompressed_data);
                &mut uncompressed_cursor
            }
            _ => {
                bail!("Internal logic error (invalid block type for hierarchy)");
            }
        };

        let mut tree = espalier::Tree::with_capacity(num_scopes_hint);

        let mut first = true;
        let mut next_varid = 0;

        loop {
            let tag = compressed_reader.read_u8()?;
            if first && tag != FST_ST_VCD_SCOPE {
                bail!("First tag must be SCOPE");
            }
            first = false;
            match tag {
                FST_ST_GEN_ATTRBEGIN => {
                    let attr_type = compressed_reader.read_u8()?;
                    let attr_subtype = compressed_reader.read_u8()?;
                    let attr_name = compressed_reader.read_null_terminated_string(512)?;
                    let attr_value = compressed_reader.read_varint()?;

                    // TODO: Record attributes.

                    info!("Attribute: {attr_name} = {attr_value}");
                }
                FST_ST_GEN_ATTREND => {}
                FST_ST_VCD_SCOPE => {
                    let scope_type = compressed_reader.read_u8()?;
                    let scope_name = compressed_reader.read_null_terminated_string(512)?;
                    let scope_component = compressed_reader.read_null_terminated_string(512)?;

                    tree.push(HierarchyScope {
                        type_: scope_type,
                        name: scope_name,
                        component: scope_component,
                        vars: Vec::new(),
                        attrs: Vec::new(),
                    });
                }
                FST_ST_VCD_UPSCOPE => {
                    if tree.up().is_none() {
                        break;
                    }
                }
                var_type => {
                    let var_direction = compressed_reader.read_u8()?;
                    let var_name = compressed_reader.read_null_terminated_string(512)?;
                    let var_length = compressed_reader.read_varint()?;
                    let var_alias = compressed_reader.read_varint()?;

                    info!("  Var {var_name:?} length {var_length}");

                    let id = if var_alias == 0 {
                        // Not an alias.
                        let id = next_varid;
                        next_varid += 1;
                        id
                    } else {
                        // Alias.
                        var_alias - 1
                    };

                    let current_scope = tree.last_mut().unwrap();

                    current_scope.value.vars.push(HierarchyVar {
                        type_: var_type,
                        direction: var_direction,
                        name: var_name,
                        length: var_length,
                        id: VarId(id as usize),
                        is_alias: var_alias != 0,
                    });
                }
            }
        }

        // TODO: Verify we are at the end.

        // Restore the position at the end of the compressed block, otherwise
        // the block reader complains.
        reader.seek(SeekFrom::Start(start_pos + block_length))?;

        Ok(tree)
    }

    fn read_value_change_block(
        reader: &mut (impl BufRead + Seek),
        block_length: u64,
        num_vars: u64,
        var_data: &mut TiVec<VarId, VarData>,
    ) -> Result<ValueChangeBlockData> {
        // File is at `vc_start_time`.

        // Record the offset of the end of the block.
        let block_end = reader.stream_position()? + block_length;

        let start_time = reader.read_u64::<BigEndian>()?;
        let end_time = reader.read_u64::<BigEndian>()?;
        let memory_required = reader.read_u64::<BigEndian>()?;
        let bits_uncompressed_length = reader.read_varint()?;

        let bits_compressed_length = reader.read_varint()?;
        let bits_count = reader.read_varint()?;
        let bits_data_offset = reader.stream_position()?;

        // seek_relative() may be more efficient but it probably doesn't really
        // matter here.
        reader.seek(SeekFrom::Current(bits_compressed_length.try_into()?))?;

        let waves_count = reader.read_varint()?;
        let waves_packtype = reader.read_u8()?;
        let waves_data_offset = reader.stream_position()?;

        // There's no waves_uncompressed_length so now we have to read back from the end of the block.
        reader.seek(SeekFrom::Start(
            block_end
                .checked_sub(24)
                .context("Value Change time_uncompressed_length offset")?,
        ))?;

        let time_uncompressed_length = reader.read_u64::<BigEndian>()?;
        let time_compressed_length = reader.read_u64::<BigEndian>()?;
        let time_count = reader.read_u64::<BigEndian>()?;

        let position_length_offset = block_end
            .checked_sub(time_compressed_length + 32)
            .context("Value Change position_length_offset")?;
        let time_data_offset = position_length_offset + 8;

        reader.seek(SeekFrom::Start(position_length_offset))?;

        let position_length = reader.read_u64::<BigEndian>()?;

        let position_data_offset = position_length_offset
            .checked_sub(position_length)
            .context("Value Change position_data_offset")?;

        reader.seek(SeekFrom::Start(position_data_offset))?;

        // Read the waves offsets and lengths and add them to var_data.
        let waves_data_length = position_data_offset
            .checked_sub(waves_data_offset)
            .context("Invalid Value Change block")?;

        Self::read_wave_slices(reader, num_vars, var_data, waves_data_length)?;

        reader.seek(SeekFrom::Start(time_data_offset))?;

        // Read the times.
        let times = Self::read_change_times(
            reader,
            time_compressed_length,
            time_uncompressed_length,
            time_count,
        )?;

        // Seek to the next block.
        reader.seek(SeekFrom::Start(block_end))?;

        Ok(ValueChangeBlockData {
            info: ValueChangeBlockInfo {
                start_time,
                end_time,
                memory_required,
                bits_uncompressed_length,
                bits_compressed_length,
                bits_count,
                bits_data_offset,
                waves_count,
                waves_packtype,
                waves_data_offset,
                position_data_offset,
                position_length,
                time_data_offset,
                time_uncompressed_length,
                time_compressed_length,
                time_count,
            },
            times,
        })
    }

    fn read_geometry_block(
        reader: &mut (impl BufRead + Seek),
        block_length: u64,
    ) -> Result<VarLengths> {
        let uncompressed_length = reader.read_u64::<BigEndian>()?;
        let count = reader.read_u64::<BigEndian>()?;

        let compressed_length = block_length
            .checked_sub(16)
            .context("Invalid geometry block length")?;

        let start_pos = reader.stream_position()?;

        let mut bufreader;

        // If the compressed length is the same as the
        // uncompressed length then it isn't compressed.
        let mut compressed_reader: &mut dyn Read = if uncompressed_length == compressed_length {
            reader
        } else {
            bufreader = BufReader::new(ZlibDecoder::new(&mut *reader));
            &mut bufreader
        };

        let mut var_lengths = VarLengths {
            lengths: TiVec::with_capacity(count as usize),
            lengths_long: HashMap::new(),
        };

        for varid in 0..count {
            let length = compressed_reader.read_varint()?;
            if length == 0 {
                // It's a real (always 8 bytes).
                var_lengths.lengths.push(VAR_LENGTH_REAL);
            } else if length == 0xFFFFFFFF {
                // Zero length.
                var_lengths.lengths.push(0);
            } else if length >= VAR_LENGTH_REAL as u64 {
                var_lengths.lengths.push(VAR_LENGTH_LONG);
                var_lengths.lengths_long.insert(
                    VarId(varid as usize),
                    length
                        .try_into()
                        .context("Variable has an insane number of bits")?,
                );
            } else {
                var_lengths.lengths.push(length as u8);
            }
        }

        // Restore the position at the end of the compressed block, otherwise
        // the block reader complains.
        reader.seek(SeekFrom::Start(start_pos + compressed_length))?;

        Ok(var_lengths)
    }

    fn read_blackout_block(reader: &mut (impl BufRead + Seek)) -> Result<Vec<(BlackoutType, u64)>> {
        let count = reader.read_varint()?;

        let mut blackouts = Vec::with_capacity(count as usize);

        let mut time = 0;

        for _ in 0..count {
            let activity = if reader.read_u8()? == 0 {
                BlackoutType::DumpOff
            } else {
                BlackoutType::DumpOn
            };
            time += reader.read_varint()?;
            blackouts.push((activity, time));
        }

        Ok(blackouts)
    }

    // Hmm we can't actually do this until the end because we need the var lengths.
    fn read_bits_array(
        reader: &mut impl BufRead,
        compressed_length: u64,
        uncompressed_length: u64,
        count: u64,
        var_lengths: &VarLengths,
        var_data: &mut TiVec<VarId, VarData>,
    ) -> Result<()> {
        let mut bufreader;

        // If the compressed length is the same as the
        // uncompressed length then it isn't compressed.
        let mut reader: &mut dyn BufRead = if uncompressed_length == compressed_length {
            reader
        } else {
            bufreader = BufReader::new(ZlibDecoder::new(reader));
            &mut bufreader
        };

        for varid in 0..count as usize {
            let varid = VarId(varid);
            let length = var_lengths.length(varid);

            let value = value_from_ascii(&mut reader, length)?;

            var_data[varid].initial_values.push(value);
        }
        Ok(())
    }

    fn read_wave_slices(
        reader: &mut (impl BufRead + Seek),
        num_vars: u64,
        var_data: &mut TiVec<VarId, VarData>,
        waves_data_length: u64,
    ) -> Result<()> {
        let mut prev_non_alias_offset: u64 = 0;
        let mut prev_dynamic_alias = None;

        let num_vars = num_vars as usize;

        if var_data.len() != num_vars {
            bail!(
                "Internal error: var_data has length {} but should have length {}",
                var_data.len(),
                num_vars
            );
        }

        // The var ID of the first var that has not had its length resolved yet.
        // Once a var has had its length resolved it isn't possible that any
        // prior ones have not had their lengths resolved.
        let mut varid_length_unresolved = VarId(0);

        let mut varid = VarId(0);
        while varid.0 < num_vars {
            // Lowest bit indicates varint / svarint.
            let mut varint_bytes = [0; 10];
            let mut varint_length = 0;
            loop {
                let byte = reader.read_u8()?;
                varint_bytes[varint_length] = byte;
                varint_length += 1;
                if byte & 0x80 == 0 {
                    break;
                }
                if varint_length >= varint_bytes.len() {
                    bail!("Invalid varint");
                }
            }

            let varint_bytes = &varint_bytes[0..varint_length];

            if varint_bytes[0] & 0x01 == 0 {
                // This is a varint encoding a run of zeros, equal to `run_length << 1`.
                let zero_run_length =
                    decode_varint(&varint_bytes).context("Varint decode error")? >> 1;

                for _ in 0..zero_run_length {
                    var_data[varid].wave_slices.push(0..0);
                    varid.0 += 1;
                }
                continue;
            }

            // This is an svarint encoding a value, equal to `value << 1 | 1`.
            let value = decode_svarint(&varint_bytes).context("Varint decode error")? >> 1;

            // The value means:
            //   0:  Equal to the previous dynamic alias.
            //   <0: A dynamic alias to a previous variable.
            //   >0: An offset into the waves (encoded as a delta from the previous offset).
            match value {
                x if x > 0 => {
                    // Delta from previous non-alias.
                    prev_non_alias_offset += x as u64;
                    // -1 because the offest in the file is from vc_waves_packtype.
                    // Use u64::MAX to mean "unresolved".
                    var_data[varid]
                        .wave_slices
                        .push(prev_non_alias_offset - 1..u64::MAX);

                    // Resolve the previous var and any aliases to it.
                    for v in varid_length_unresolved.0..varid.0 {
                        let last = var_data[VarId(v)].wave_slices.last_mut().unwrap();
                        if last.end == u64::MAX {
                            // It's unresolved. -1 because the offest in the file is from vc_waves_packtype.
                            last.end = prev_non_alias_offset - 1;
                        }
                    }
                    varid_length_unresolved = varid;
                }
                x if x < 0 => {
                    // Dynamic alias to another variable (must be lower than the current one).
                    let aliased_var = VarId((-(x + 1)) as usize);
                    if aliased_var.0 >= varid.0 {
                        bail!("Position table aliases var {varid:?} to {aliased_var:?} which has not been seen yet.");
                    }
                    prev_dynamic_alias = Some(aliased_var);
                    let aliased_var_wave_slice = var_data[aliased_var]
                        .wave_slices
                        .last()
                        .expect("Aliased var has no offset")
                        .clone();
                    var_data[varid].wave_slices.push(aliased_var_wave_slice);
                }
                _ => {
                    // Same as the previous dynamic alias.
                    if let Some(aliased_var) = prev_dynamic_alias {
                        let aliased_var_wave_slice = var_data[aliased_var]
                            .wave_slices
                            .last()
                            .expect("Aliased var has no offset")
                            .clone();
                        var_data[varid].wave_slices.push(aliased_var_wave_slice);
                    } else {
                        bail!("Position table aliased var to previous alias but there is none.");
                    }
                }
            }
            varid.0 += 1;
        }

        // Resolve final lengths using the total length.
        for v in varid_length_unresolved.0..num_vars {
            let last = var_data[VarId(v)].wave_slices.last_mut().unwrap();
            if last.end == u64::MAX {
                // It's unresolved.
                last.end = waves_data_length;
            }
        }

        Ok(())
    }

    fn read_change_times(
        reader: &mut (impl BufRead + Seek),
        compressed_length: u64,
        uncompressed_length: u64,
        count: u64,
    ) -> Result<Vec<u64>> {
        let mut times = Vec::with_capacity(count as usize);

        let mut time = 0;

        // If the compressed length is different to the uncompressed length then it's compressed.
        if uncompressed_length != compressed_length {
            // Compressed with ZLib.
            let mut decoder = ZlibDecoder::new(reader);

            for n in 0..count {
                time += decoder
                    .read_varint()
                    .with_context(|| format!("Reading compressed time table value {n}"))?;
                times.push(time);
            }
        } else {
            for _ in 0..count {
                time += reader.read_varint()?;
                times.push(time);
            }
        }
        info!("Read change times: {:?}", times);
        Ok(times)
    }
}

/// Read a value from packed bits that only contains 0s and 1s.
fn value_from_packed_bits(reader: &mut impl BufRead, bits: u32) -> Result<Value> {
    let bytes_2 = (bits + 7) / 8;
    let packed_bits = reader.read_tinyvec::<16>(bytes_2 as usize)?;

    let mut val = Value::default();
    let bytes_4 = (bits + 3) / 4;

    val.0.reserve(bytes_4 as usize);

    // Interleave the bits with 0s.
    // https://graphics.stanford.edu/~seander/bithacks.html#Interleave64bitOps
    // But I'll just do it the obvious way.
    for b in packed_bits.iter() {
        val.0.push(
            ((b & 0b1000) << 2) | ((b & 0b0100) << 2) | ((b & 0b0010) << 1) | ((b & 0b0001) << 0),
        );
        val.0.push(
            ((b & 0b1000_0000) >> 1)
                | ((b & 0b0100_0000) >> 2)
                | ((b & 0b0010_0000) >> 3)
                | ((b & 0b0001_0000) >> 4),
        );
    }
    Ok(val)
}

fn value_from_ascii(reader: &mut impl BufRead, var_length: VarLength) -> Result<Value> {
    Ok(match var_length {
        VarLength::Bits(bits) => {
            let bits = bits as usize;

            let buffer = reader.read_tinyvec::<64>(bits)?;

            let (has_xz, other) = buffer.iter().fold((false, false), |acc, &c| match c {
                b'0' | b'1' => acc,
                b'x' | b'X' | b'z' | b'Z' => (true, acc.1),
                _ => (acc.0, true),
            });

            if other {
                bail!("Value contains a bit that isn't 0, 1, X or Z. This isn't supported.");
            }

            info!("Reading {} bit value", bits);

            let mut val = Value::default();

            let bytes = (bits + 3) / 4;

            val.0.resize(bytes, 0);

            for (i, &c) in buffer.iter().enumerate() {
                // The only possible characters are:
                // b'0' = 0x30 = 0b....0000
                // b'1' = 0x31 = 0b....0001
                // b'X' = 0x58 = 0b....1000
                // b'x' = 0x78 = 0b....1000
                // b'Z' = 0x5A = 0b....1010
                // b'z' = 0x7A = 0b....1010

                // So we can make the value we want (0, 1, 2, 3) through:
                //
                //   (c | (c >> 1) | (c >> 2)) & 0x02
                let b = ((c | (c >> 1) | (c >> 2)) & 0x02) as u8;
                val.0[i / 4] |= b << ((i % 4) * 2) as u8;
            }

            val
        }
        VarLength::Real => {
            // TODO: Handle endianness.
            let todo = reader.read_f64::<LittleEndian>()?;
            todo!()
        }
    })
}

fn value_and_time_index_delta_from_waves_table(
    reader: &mut impl BufRead,
    var_length: VarLength,
) -> Result<(Value, u64)> {
    Ok(match var_length {
        VarLength::Bits(1) => {
            // Special encoding - the value and time index delta are encoded in a single varint.
            let varint = reader.read_varint()?;
            if varint & 0b01 == 0 {
                let time_index_delta = varint >> 2;
                // 0 or 1
                if varint & 0b10 == 0 {
                    (Value(tiny_vec!([u8; 16] => 0)), time_index_delta)
                } else {
                    (Value(tiny_vec!([u8; 16] => 1)), time_index_delta)
                }
            } else {
                let time_index_delta = varint >> 4;
                // X, Z, etc.
                match varint & 0b1110 {
                    0b0000 => (Value(tiny_vec!([u8; 16] => 2)), time_index_delta), // X
                    0b0010 => (Value(tiny_vec!([u8; 16] => 3)), time_index_delta), // Z
                    _ => bail!("Values other than 0, 1, X, Z are not supported."),
                }
            }
        }
        VarLength::Bits(bits) => {
            let time_index_delta_and_is_binary = reader.read_varint()?;
            let time_index_delta = time_index_delta_and_is_binary >> 1;
            let is_binary = (time_index_delta_and_is_binary & 1) == 0;

            let value = if is_binary {
                // Raw bits packed into bytes.
                value_from_packed_bits(reader, bits)?
            } else {
                // Encoded as raw ASCII.
                value_from_ascii(reader, var_length)?
            };
            (value, time_index_delta)
        }
        VarLength::Real => {
            todo!()
        }
    })
}

// impl Waves for Fst {
//     fn hierarchy(&self) -> &super::Hierarchy {
//         todo!()
//     }

//     fn load_waves(&mut self, variable_ids: std::collections::HashSet<usize>) -> Result<()> {
//         todo!()
//     }

//     fn wave(&self, variable_id: super::VariableId) -> Result<&super::Wave<u8>> {
//         todo!()
//     }

//     fn times(&self) -> &[u64] {
//         todo!()
//     }

//     fn timebase_order(&self) -> i8 {
//         todo!()
//     }

//     fn variable_info(&self, variable_id: super::VariableId) -> Result<&super::VariableInfo> {
//         todo!()
//     }
// }

#[cfg(test)]
mod test {
    use super::*;

    fn logging_setup() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .init();
    }

    #[test]
    fn test_reading_file() {
        logging_setup();

        let file = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../samples/hdl-example.fst"
        ));

        let mut fst = Fst::load(file).unwrap();

        // dbg!(fst.header.num_vars);
        for varid in [7] {
            // 0..fst.header.num_vars {
            let wave = fst.read_wave(VarId(varid as usize)).unwrap();
            dbg!(&varid, &wave);
        }
    }
}
