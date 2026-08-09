use std::mem;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::format::cursor::Cursor;
use crate::limits::{
    MemoryBudget, checked_usize, ensure_u64_limit, ensure_usize_limit, try_reserve_vec,
};
use crate::source::FileSource;
use crate::types::{Header, Limits};

#[derive(Debug, Clone)]
pub struct RecordBlockInfo {
    pub comp_offset: u64,
    pub comp_size: u64,
    pub decomp_offset: u64,
    pub decomp_size: u64,
}

#[derive(Debug, Clone)]
pub struct RecordIndex {
    pub num_entries: u64,
    pub total_decompressed_len: u64,
    pub blocks: Vec<RecordBlockInfo>,
    pub retained_bytes: usize,
}

impl RecordIndex {
    pub fn find_block(&self, record_offset: u64) -> Option<usize> {
        let mut left = 0usize;
        let mut right = self.blocks.len();
        while left < right {
            let mid = (left + right) / 2;
            let block = &self.blocks[mid];
            let end = block.decomp_offset.checked_add(block.decomp_size)?;
            if record_offset < block.decomp_offset {
                right = mid;
            } else if record_offset >= end {
                left = mid + 1;
            } else {
                return Some(mid);
            }
        }
        None
    }
}

pub fn parse_record_index(
    source: &FileSource,
    header: &Header,
    record_section_offset: u64,
    limits: &Limits,
    memory: &Arc<MemoryBudget>,
) -> Result<RecordIndex> {
    if !header.is_v2() {
        return Err(Error::Unsupported(
            "MDict format major version other than 2",
        ));
    }

    let _header_memory = memory.reserve(32, "record section header")?;
    let header_bytes = source.read_exact_at(record_section_offset, 32, "record section header")?;
    let mut cursor = Cursor::new(&header_bytes);
    let num_blocks = cursor.read_u64_be("record num_blocks")?;
    let num_entries = cursor.read_u64_be("record num_entries")?;
    let index_len = cursor.read_u64_be("record index length")?;
    let blocks_len = cursor.read_u64_be("record blocks length")?;

    let (num_blocks, index_len_usize) =
        validate_record_header(num_blocks, num_entries, index_len, limits)?;
    let index_offset = record_section_offset
        .checked_add(32)
        .ok_or(Error::InvalidFormat("record index offset overflow"))?;
    let blocks_offset = index_offset
        .checked_add(index_len)
        .ok_or(Error::InvalidFormat("record blocks offset overflow"))?;
    source.ensure_range(index_offset, index_len, "record block index")?;
    source.ensure_range(blocks_offset, blocks_len, "record block section")?;

    let _index_memory = memory.reserve(index_len_usize, "record block index")?;
    let retained_bytes = num_blocks
        .checked_mul(mem::size_of::<RecordBlockInfo>())
        .ok_or(Error::InvalidFormat("record metadata size overflow"))?;
    let _metadata_memory = memory.reserve(retained_bytes, "record block metadata")?;
    let index_bytes = source.read_exact_at(index_offset, index_len_usize, "record block index")?;
    let mut cursor = Cursor::new(&index_bytes);
    let mut blocks = Vec::new();
    try_reserve_vec(&mut blocks, num_blocks, "record block metadata")?;
    let mut comp_offset = blocks_offset;
    let mut decomp_offset = 0u64;
    let mut total_comp = 0u64;

    for _ in 0..num_blocks {
        let comp_size = cursor.read_u64_be("record block compressed size")?;
        let decomp_size = cursor.read_u64_be("record block decompressed size")?;
        validate_record_block_sizes(comp_size, decomp_size, limits)?;
        source.ensure_range(comp_offset, comp_size, "record block")?;
        let next_comp_offset = comp_offset
            .checked_add(comp_size)
            .ok_or(Error::InvalidFormat("record block offset overflow"))?;
        let next_decomp_offset = decomp_offset
            .checked_add(decomp_size)
            .ok_or(Error::InvalidFormat("record block offset overflow"))?;
        blocks.push(RecordBlockInfo {
            comp_offset,
            comp_size,
            decomp_offset,
            decomp_size,
        });
        comp_offset = next_comp_offset;
        decomp_offset = next_decomp_offset;
        total_comp = total_comp
            .checked_add(comp_size)
            .ok_or(Error::InvalidFormat("record block length overflow"))?;
    }

    if total_comp != blocks_len {
        return Err(Error::InvalidData(format!(
            "record blocks length mismatch: header={blocks_len}, summed={total_comp}"
        )));
    }
    if !cursor.is_empty() {
        return Err(Error::InvalidData(format!(
            "record block index has {} trailing bytes",
            cursor.remaining()
        )));
    }
    let expected_blocks_end = blocks_offset
        .checked_add(blocks_len)
        .ok_or(Error::InvalidFormat("record block section end overflow"))?;
    if comp_offset != expected_blocks_end {
        return Err(Error::InvalidData(
            "record block ranges do not cover the declared section".to_owned(),
        ));
    }

    Ok(RecordIndex {
        num_entries,
        total_decompressed_len: decomp_offset,
        blocks,
        retained_bytes,
    })
}

fn validate_record_header(
    num_blocks: u64,
    _num_entries: u64,
    index_len: u64,
    limits: &Limits,
) -> Result<(usize, usize)> {
    let expected_index_len = num_blocks
        .checked_mul(16)
        .ok_or(Error::InvalidFormat("record index length overflow"))?;
    if index_len != expected_index_len {
        return Err(Error::InvalidData(format!(
            "record index length mismatch: header={index_len}, expected={expected_index_len}"
        )));
    }
    ensure_u64_limit("record_index_bytes", index_len, limits.record_index_bytes)?;
    let index_len = checked_usize(index_len, "record index length")?;
    let num_blocks = checked_usize(num_blocks, "record block count")?;
    let metadata_bytes = num_blocks
        .checked_mul(mem::size_of::<RecordBlockInfo>())
        .ok_or(Error::InvalidFormat("record block metadata size overflow"))?;
    ensure_usize_limit(
        "record_block_metadata_bytes",
        metadata_bytes,
        limits.block_metadata_bytes,
    )?;
    Ok((num_blocks, index_len))
}

fn validate_record_block_sizes(comp_size: u64, decomp_size: u64, limits: &Limits) -> Result<()> {
    ensure_u64_limit(
        "record_block_compressed_bytes",
        comp_size,
        limits.compressed_block_bytes,
    )?;
    ensure_u64_limit(
        "record_block_decompressed_bytes",
        decomp_size,
        limits.decompressed_block_bytes,
    )?;
    if comp_size < 8 {
        return Err(Error::InvalidData(format!(
            "record block is {comp_size} bytes; at least 8 are required"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_exact_record_index_length() {
        let error = validate_record_header(2, 1, 31, &Limits::new()).unwrap_err();
        assert!(matches!(error, Error::InvalidData(_)));
    }

    #[test]
    fn rejects_oversized_record_blocks_before_they_can_be_read() {
        let limits = Limits::new();
        let oversized = u64::try_from(limits.decompressed_block_bytes).unwrap() + 1;
        let error = validate_record_block_sizes(8, oversized, &limits).unwrap_err();
        assert!(matches!(error, Error::LimitExceeded { .. }));
    }
}
