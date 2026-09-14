use std::{path::PathBuf, str::FromStr};

use capnp::{
    message::{self, ReaderOptions},
    serialize,
};
use stigmerge_fileindex::{FileSpec, Index, PayloadPiece, PayloadSlice};

use super::{stigmerge_capnp::index, Decoder, Digest, Encoder, Error, Result, MAX_INDEX_BYTES};

impl Encoder for Index {
    /// Encode the portable fields (file and payload specs) of an Index to a binary representation with capnp.
    ///
    /// Index::root is not encoded, as it's specific to an Index's application
    /// to the local filesystem.
    fn encode(&self) -> Result<Vec<u8>> {
        let mut builder = message::Builder::new_default();
        let mut index_builder = builder.get_root::<index::Builder>()?;

        // Encode the pieces
        let mut pieces_builder = index_builder
            .reborrow()
            .init_pieces(self.payload().pieces().len() as u32);
        for (i, idx_piece) in self.payload().pieces().iter().enumerate() {
            let mut piece_builder = pieces_builder.reborrow().get(i as u32);
            let mut piece_digest_builder = piece_builder.reborrow().init_digest();
            let piece_digest = idx_piece.digest();
            piece_digest_builder.set_p0(u64::from_be_bytes(piece_digest[0..8].try_into()?));
            piece_digest_builder.set_p1(u64::from_be_bytes(piece_digest[8..16].try_into()?));
            piece_digest_builder.set_p2(u64::from_be_bytes(piece_digest[16..24].try_into()?));
            piece_digest_builder.set_p3(u64::from_be_bytes(piece_digest[24..32].try_into()?));
            piece_builder.set_length(idx_piece.length() as u32);
        }

        // Encode the files
        let mut files_builder = index_builder
            .reborrow()
            .init_files(self.files().len() as u32);
        for (i, idx_file) in self.files().iter().enumerate() {
            let mut file_builder = files_builder.reborrow().get(i as u32);
            let mut slice_builder = file_builder.reborrow().init_contents();
            slice_builder.set_starting_piece(idx_file.contents().starting_piece() as u32);
            slice_builder.set_piece_offset(idx_file.contents().piece_offset() as u32);
            slice_builder.set_length(idx_file.contents().length() as u64);
            file_builder.set_path(
                idx_file
                    .path()
                    .as_os_str()
                    .to_str()
                    .ok_or(Error::EncodePath(idx_file.path().to_owned()))?,
            );
        }

        let message = serialize::write_message_segments_to_words(&builder);
        if message.len() > MAX_INDEX_BYTES {
            return Err(Error::IndexTooLarge(message.len()));
        }
        Ok(message)
    }
}

impl Decoder for (Vec<PayloadPiece>, Vec<FileSpec>) {
    fn decode(buf: &[u8]) -> Result<Self> {
        let reader = serialize::read_message(buf, ReaderOptions::new())?;
        let index_reader = reader.get_root::<index::Reader>()?;
        let pieces_reader = index_reader.get_pieces()?;
        let files_reader = index_reader.get_files()?;

        let mut idx_pieces = vec![];
        for piece in pieces_reader.iter() {
            let mut piece_digest = Digest::default();
            let piece_digest_reader = piece.get_digest()?;
            piece_digest[0..8].clone_from_slice(&piece_digest_reader.get_p0().to_be_bytes()[..]);
            piece_digest[8..16].clone_from_slice(&piece_digest_reader.get_p1().to_be_bytes()[..]);
            piece_digest[16..24].clone_from_slice(&piece_digest_reader.get_p2().to_be_bytes()[..]);
            piece_digest[24..32].clone_from_slice(&piece_digest_reader.get_p3().to_be_bytes()[..]);
            idx_pieces.push(PayloadPiece::new(piece_digest, piece.get_length() as usize));
        }

        let mut idx_files = vec![];
        for file in files_reader.iter() {
            let payload_slice_reader = file.get_contents()?;
            let spec = FileSpec::new(
                PathBuf::from_str(file.get_path()?.to_str()?)
                    .map_err(|e| Error::Other(format!("{:?}", e)))?,
                PayloadSlice::new(
                    payload_slice_reader.get_starting_piece() as usize,
                    payload_slice_reader.get_piece_offset() as usize,
                    payload_slice_reader.get_length() as usize,
                ),
            );
            // DUCAT modification (see ../../../STIGMERGE-NOTICE.md): the
            // publisher chose this path, and the fetcher would create and
            // write it under its root before verifying a byte. An absolute
            // path replaces the root outright; a `..` walks out of it.
            if !spec.is_contained() {
                return Err(Error::UnsafePath(spec.path().to_owned()));
            }
            idx_files.push(spec);
        }

        // DUCAT modification (see ../../../STIGMERGE-NOTICE.md): and the
        // numbers beside the paths are the publisher's too. Upstream took
        // every piece and slice length as read — so an index could declare a
        // 900 GB file, point a slice past the end of its own pieces list
        // (panicking the verifier), or hand one piece to two files. Every
        // rule here holds for every index this stack produces; none of them
        // can be true of content anybody actually indexed. Refused whole,
        // before `Indexer::from_wanted` creates and sizes a single file.
        //
        // The payload length lives in the share header, not here, so the
        // total is checked again where the two meet (`record::read_index`).
        stigmerge_fileindex::check_index_shape(&idx_pieces, &idx_files, None)?;

        Ok((idx_pieces, idx_files))
    }
}

#[cfg(test)]
mod tests {
    use crate::proto::{Decoder, Header};

    use super::*;

    use std::io::Write;
    use stigmerge_fileindex::{Index, Indexer, PayloadSpec};
    use tempfile::NamedTempFile;

    fn temp_file(pattern: u8, count: usize) -> NamedTempFile {
        let mut tempf = NamedTempFile::new().expect("temp file");
        let contents = vec![pattern; count];
        tempf.write(contents.as_slice()).expect("write temp file");
        tempf
    }

    #[tokio::test]
    async fn round_trip_index() {
        let tempf = temp_file(b'@', 4194304);
        let indexer = Indexer::from_file(tempf.path()).await.expect("indexer");
        let idx = indexer.index().await.expect("index");
        let header = Header::new(
            idx.payload().digest().try_into().expect("digest fits"),
            idx.payload().length(),
            1,
            &[],
            None,
            None,
        );
        let message = idx.encode().expect("encode index");
        let (payload_pieces, payload_files) =
            <(Vec<PayloadPiece>, Vec<FileSpec>)>::decode(message.as_slice())
                .expect("decode payload pieces and files");
        let idx2 = Index::new(
            tempf.path().canonicalize().unwrap().parent().unwrap().to_owned(),
            PayloadSpec::new(
                header.payload_digest(),
                header.payload_length(),
                payload_pieces,
            ),
            payload_files,
        );
        assert_eq!(idx, idx2);
    }

    /// An index off the wire that names a path outside its root is refused
    /// at decode — before anything could be created under that name.
    #[tokio::test]
    async fn escaping_paths_are_refused_at_decode() {
        let tempf = temp_file(b'@', 65536);
        let indexer = Indexer::from_file(tempf.path()).await.expect("indexer");
        let idx = indexer.index().await.expect("index");
        for bad in [
            "../../shared_prefs/ducat_sites.xml",
            "/data/data/org.ducatproject.ducat/files/x",
            "a/../../b",
            "",
        ] {
            let evil = Index::new(
                idx.root().to_owned(),
                idx.payload().clone(),
                vec![FileSpec::new(
                    PathBuf::from(bad),
                    idx.files()[0].contents(),
                )],
            );
            let message = evil.encode().expect("encode index");
            let err = <(Vec<PayloadPiece>, Vec<FileSpec>)>::decode(message.as_slice())
                .expect_err(bad);
            assert!(
                matches!(err, Error::UnsafePath(_)),
                "{bad:?} refused for the wrong reason: {err}"
            );
        }
        // And the honest shape still decodes.
        let message = idx.encode().expect("encode index");
        <(Vec<PayloadPiece>, Vec<FileSpec>)>::decode(message.as_slice()).expect("plain path");
    }

    /// DUCAT modification (see ../../../STIGMERGE-NOTICE.md): an index off
    /// the wire that declares a shape nobody could have indexed is refused
    /// at decode — before `Indexer::from_wanted` creates and sizes a file
    /// for every name in it.
    #[tokio::test]
    async fn hostile_shapes_are_refused_at_decode() {
        use stigmerge_fileindex::{PayloadSlice, PIECE_SIZE_BYTES};

        let root = PathBuf::from("/tmp");
        let honest_piece = PayloadPiece::new([1u8; 32], PIECE_SIZE_BYTES);

        // "One file, 900 GB", on one piece: the shape D2 is about. The
        // fetcher would have created and sized that file.
        let colossal = Index::new(
            root.clone(),
            PayloadSpec::new([2u8; 32], 900_000_000_000, vec![honest_piece.clone()]),
            vec![FileSpec::new(
                PathBuf::from("film.mkv"),
                PayloadSlice::new(0, 0, 900_000_000_000),
            )],
        );
        let err = <(Vec<PayloadPiece>, Vec<FileSpec>)>::decode(
            colossal.encode().expect("encode").as_slice(),
        )
        .expect_err("900 GB on one piece");
        assert!(matches!(err, Error::UnsafeIndex(_)), "{err}");

        // A piece longer than a piece.
        let fat = Index::new(
            root.clone(),
            PayloadSpec::new(
                [2u8; 32],
                PIECE_SIZE_BYTES * 64,
                vec![PayloadPiece::new([1u8; 32], PIECE_SIZE_BYTES * 64)],
            ),
            vec![FileSpec::new(
                PathBuf::from("a.bin"),
                PayloadSlice::new(0, 0, PIECE_SIZE_BYTES * 64),
            )],
        );
        let err =
            <(Vec<PayloadPiece>, Vec<FileSpec>)>::decode(fat.encode().expect("encode").as_slice())
                .expect_err("a 64 MiB piece");
        assert!(matches!(err, Error::UnsafeIndex(_)), "{err}");

        // A slice that starts past the end of the pieces list: the panic in
        // the verifier, arriving as bytes.
        let past = Index::new(
            root.clone(),
            PayloadSpec::new([2u8; 32], PIECE_SIZE_BYTES, vec![honest_piece.clone()]),
            vec![FileSpec::new(
                PathBuf::from("a.bin"),
                PayloadSlice::new(4096, 0, PIECE_SIZE_BYTES),
            )],
        );
        let err =
            <(Vec<PayloadPiece>, Vec<FileSpec>)>::decode(past.encode().expect("encode").as_slice())
                .expect_err("a slice past the pieces");
        assert!(matches!(err, Error::UnsafeIndex(_)), "{err}");

        // And the honest one-piece-one-file index still decodes.
        let honest = Index::new(
            root,
            PayloadSpec::new([2u8; 32], PIECE_SIZE_BYTES, vec![honest_piece]),
            vec![FileSpec::new(
                PathBuf::from("a.bin"),
                PayloadSlice::new(0, 0, PIECE_SIZE_BYTES),
            )],
        );
        <(Vec<PayloadPiece>, Vec<FileSpec>)>::decode(
            honest.encode().expect("encode").as_slice(),
        )
        .expect("an honest index");
    }
}
