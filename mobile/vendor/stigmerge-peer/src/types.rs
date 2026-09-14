use std::path::PathBuf;

use stigmerge_fileindex::{Index, BLOCK_SIZE_BYTES, PIECE_SIZE_BLOCKS, PIECE_SIZE_BYTES};
use veilid_core::{RecordKey, RouteId};

use crate::{piece_map::PieceMap, proto::Header};

#[derive(Debug, Clone)]
pub struct LocalShareInfo {
    pub key: RecordKey,
    pub header: Header,
    pub want_index: Index,
    pub want_index_digest: [u8; 32],
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RemoteShareInfo {
    pub key: RecordKey,
    pub header: Header,
    pub index: Index,
    pub index_digest: [u8; 32],
    pub route_id: RouteId,
    pub have_map: PieceMap,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileBlockFetch {
    pub file_index: usize,
    pub piece_index: usize,
    pub piece_offset: usize,
    pub block_index: usize,
}

impl FileBlockFetch {
    /// Offset of this block within its own file, given the file's starting
    /// piece in the payload. With single-file shares starting_piece is 0
    /// and this is the old payload-global math unchanged; with multi-file
    /// shares (piece-aligned slices) it is what makes the seek land inside
    /// the right file instead of a gigabyte past its end.
    ///
    /// DUCAT modification (see ../STIGMERGE-NOTICE.md): checked, and
    /// `None` rather than a panic or a wrapped offset. Every term comes
    /// from an index a stranger wrote: a `starting_piece` past this block's
    /// piece underflowed the subtraction (a colossal offset in release, a
    /// panic in debug), and the multiplication overflows on a 32-bit target
    /// for any index that names a big enough piece.
    pub fn block_offset_in_file(&self, starting_piece: usize) -> Option<u64> {
        let rel = self.piece_index.checked_sub(starting_piece)?;
        (rel as u64)
            .checked_mul(PIECE_SIZE_BYTES as u64)?
            .checked_add(self.piece_offset as u64)?
            .checked_add((self.block_index as u64).checked_mul(BLOCK_SIZE_BYTES as u64)?)
    }
}

/// How many bytes the block at `block_index` of a piece of `piece_length`
/// bytes holds — `None` if that block is not in that piece at all.
///
/// DUCAT modification (see ../STIGMERGE-NOTICE.md): the one arithmetic both
/// ends of a block transfer must agree on, in one place. The seeder used to
/// seek to the block and reply with whatever a short read returned; the
/// fetcher used to take whatever came back and clamp it to a whole block.
/// Between them, the last block of a piece — the short one — could be
/// answered with a full block of garbage, and the file grew past the length
/// its own index declares. The index says how long this block is; both ends
/// ask it.
pub fn expected_block_len(piece_length: usize, block_index: usize) -> Option<usize> {
    let consumed = block_index.checked_mul(BLOCK_SIZE_BYTES)?;
    match piece_length.checked_sub(consumed) {
        Some(left) if left > 0 => Some(left.min(BLOCK_SIZE_BYTES)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceState {
    pub file_index: usize,
    pub piece_index: usize,
    pub piece_offset: usize,
    pub block_count: usize,
    pub blocks: u32,
}

impl PieceState {
    pub fn new(
        file_index: usize,
        piece_index: usize,
        piece_offset: usize,
        block_count: usize,
        block_index: usize,
    ) -> PieceState {
        PieceState {
            file_index,
            piece_index,
            piece_offset,
            block_count,
            // DUCAT modification (see ../STIGMERGE-NOTICE.md): checked. A
            // piece holds at most 32 blocks, so a block index of 32 or
            // more is a shift off the end of the word — a panic in debug,
            // a wrap in release. Nothing is marked present instead.
            blocks: 1u32.checked_shl(block_index as u32).unwrap_or(0),
        }
    }

    pub fn empty(
        file_index: usize,
        piece_index: usize,
        piece_offset: usize,
        block_count: usize,
    ) -> PieceState {
        PieceState {
            file_index,
            piece_index,
            piece_offset,
            block_count,
            blocks: 0u32,
        }
    }

    pub fn key(&self) -> (usize, usize) {
        (self.file_index, self.piece_index)
    }

    pub fn is_complete(&self) -> bool {
        match self.block_count {
            0 => true,
            size if size == PIECE_SIZE_BLOCKS => self.blocks == 0xffffffff,
            _ => {
                // DUCAT modification (see ../STIGMERGE-NOTICE.md): checked,
                // for the same reason as `new` above — block_count comes
                // from a piece length the publisher chose, and one over 32
                // shifted off the end of the word. A count no piece can
                // have is never complete.
                let Some(mask) = 1u32.checked_shl((self.block_count - 1) as u32) else {
                    return false;
                };
                self.blocks & mask == mask
            }
        }
    }

    pub fn merged(mut self, other: PieceState) -> PieceState {
        if self.file_index != other.file_index || self.piece_index != other.piece_index {
            panic!("attempt to merge mismatched pieces");
        }
        self.blocks |= other.blocks;
        self
    }

    pub fn cleared(mut self) -> PieceState {
        self.blocks = 0;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stigmerge_fileindex::PIECE_SIZE_BYTES;

    /// DUCAT modification (see ../STIGMERGE-NOTICE.md): a block is exactly
    /// as long as the index says, and the last one is short.
    #[test]
    fn block_lengths_come_from_the_piece() {
        // A whole piece: 32 whole blocks, and nothing after them.
        assert_eq!(
            expected_block_len(PIECE_SIZE_BYTES, 0),
            Some(BLOCK_SIZE_BYTES)
        );
        assert_eq!(
            expected_block_len(PIECE_SIZE_BYTES, PIECE_SIZE_BLOCKS - 1),
            Some(BLOCK_SIZE_BYTES)
        );
        assert_eq!(expected_block_len(PIECE_SIZE_BYTES, PIECE_SIZE_BLOCKS), None);

        // A short tail piece: one whole block, then 100 bytes. This is the
        // block a hostile mirror answered with 32 KiB of garbage.
        let tail = BLOCK_SIZE_BYTES + 100;
        assert_eq!(expected_block_len(tail, 0), Some(BLOCK_SIZE_BYTES));
        assert_eq!(expected_block_len(tail, 1), Some(100));
        assert_eq!(expected_block_len(tail, 2), None);
        assert_eq!(expected_block_len(tail, 255), None);

        // Nothing overflows on the way.
        assert_eq!(expected_block_len(1, 0), Some(1));
        assert_eq!(expected_block_len(0, 0), None);
        assert_eq!(expected_block_len(usize::MAX, usize::MAX), None);
    }

    /// A block count no piece can have is never complete, and never panics.
    #[test]
    fn absurd_block_counts_are_not_complete() {
        let s = PieceState::empty(0, 0, 0, 33);
        assert!(!s.is_complete());
        let s = PieceState::empty(0, 0, 0, usize::MAX);
        assert!(!s.is_complete());
        // And a real one still works.
        let mut s = PieceState::empty(0, 0, 0, 2);
        assert!(!s.is_complete());
        s = s.merged(PieceState::new(0, 0, 0, 2, 1));
        assert!(s.is_complete());
    }

    /// An offset that cannot be computed is `None`, not a wrapped seek.
    #[test]
    fn offsets_are_checked() {
        let b = FileBlockFetch {
            file_index: 0,
            piece_index: 3,
            piece_offset: 0,
            block_index: 0,
        };
        assert_eq!(
            b.block_offset_in_file(0),
            Some(3 * PIECE_SIZE_BYTES as u64)
        );
        // A file whose slice starts after this piece: the subtraction used
        // to underflow into a colossal offset.
        assert_eq!(b.block_offset_in_file(9), None);
    }
}
