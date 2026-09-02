//! DUCAT multi-file tests: the piece-aligned layout, pinned at its edges.
//! (See STIGMERGE-NOTICE.md — multi-file support is a DUCAT modification.)

use std::io::Write;

use tempfile::TempDir;

use super::*;

fn write_file(root: &Path, rel: &str, pattern: u8, count: usize) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).expect("mkdirs");
    let mut f = std::fs::File::create(&p).expect("create");
    f.write_all(&vec![pattern; count]).expect("write");
}

/// Three files crossing every boundary that matters — a piece and a half,
/// a sliver, exactly one piece, and an empty file — indexed as one payload
/// with every file starting on a fresh piece.
#[tokio::test]
async fn directory_index_is_piece_aligned() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    write_file(root, "a.bin", b'a', PIECE_SIZE_BYTES + PIECE_SIZE_BYTES / 2);
    write_file(root, "b/nested.bin", b'b', 10);
    write_file(root, "c.bin", b'c', PIECE_SIZE_BYTES);
    write_file(root, "z-empty.bin", b'z', 0);

    let indexer = Indexer::from_path(root).await.expect("from_path");
    let index = indexer.index().await.expect("index");

    // Sorted by absolute path under the root.
    assert_eq!(index.files().len(), 4);
    assert_eq!(index.files()[0].path(), Path::new("a.bin"));
    assert_eq!(index.files()[1].path(), Path::new("b/nested.bin"));
    assert_eq!(index.files()[2].path(), Path::new("c.bin"));
    assert_eq!(index.files()[3].path(), Path::new("z-empty.bin"));

    // Piece-aligned bases: 2 pieces, then 1, then 1, then none.
    assert_eq!(index.files()[0].contents().starting_piece(), 0);
    assert_eq!(index.files()[1].contents().starting_piece(), 2);
    assert_eq!(index.files()[2].contents().starting_piece(), 3);
    assert_eq!(index.payload().pieces().len(), 4);

    // Piece lengths: full, half, sliver, exactly-one.
    assert_eq!(index.payload().pieces()[0].length(), PIECE_SIZE_BYTES);
    assert_eq!(index.payload().pieces()[1].length(), PIECE_SIZE_BYTES / 2);
    assert_eq!(index.payload().pieces()[2].length(), 10);
    assert_eq!(index.payload().pieces()[3].length(), PIECE_SIZE_BYTES);

    // Payload length is the sum of the real bytes.
    assert_eq!(
        index.payload().length(),
        PIECE_SIZE_BYTES + PIECE_SIZE_BYTES / 2 + 10 + PIECE_SIZE_BYTES
    );

    // Piece -> file resolution.
    assert_eq!(index.file_index_for_piece(0), 0);
    assert_eq!(index.file_index_for_piece(1), 0);
    assert_eq!(index.file_index_for_piece(2), 1);
    assert_eq!(index.file_index_for_piece(3), 2);

    // A complete tree diffed against its own index wants nothing.
    let again = Indexer::from_path(root).await.expect("re-index");
    let have = again.index().await.expect("index");
    let diff = index.diff(&have);
    assert!(diff.want.is_empty(), "complete tree wants nothing");
}

/// The wanted side: only the third file exists locally. The have index must
/// land that file's pieces at the WANT's global positions, so the diff asks
/// for exactly the missing files' blocks and not one block of the present
/// one — even though an earlier file is absent entirely.
#[tokio::test]
async fn wanted_side_alignment_survives_missing_files() {
    let seed_dir = TempDir::new().expect("tempdir");
    let seed_root = seed_dir.path();
    write_file(seed_root, "a.bin", b'a', PIECE_SIZE_BYTES + 5);
    write_file(seed_root, "b.bin", b'b', 20);
    write_file(seed_root, "c.bin", b'c', PIECE_SIZE_BYTES / 2);

    let want = Indexer::from_path(seed_root)
        .await
        .expect("from_path")
        .index()
        .await
        .expect("index");

    // A fresh root holding only c.bin, byte-identical.
    let fetch_dir = TempDir::new().expect("tempdir");
    let fetch_root = fetch_dir.path();
    write_file(fetch_root, "c.bin", b'c', PIECE_SIZE_BYTES / 2);
    let want_rooted = Index::new(
        fetch_root.to_path_buf(),
        want.payload().clone(),
        want.files().clone(),
    );

    let have_indexer = Indexer::from_wanted(&want_rooted).await.expect("from_wanted");
    let have = have_indexer.index().await.expect("have index");

    let diff = want_rooted.diff(&have);
    // Wants: both pieces of a.bin and the sliver of b.bin. Not c's piece.
    assert!(
        diff.want
            .iter()
            .all(|b| b.piece_index <= 2),
        "wants stay within the missing files' pieces"
    );
    assert!(
        diff.want.iter().any(|b| b.piece_index == 0)
            && diff.want.iter().any(|b| b.piece_index == 1)
            && diff.want.iter().any(|b| b.piece_index == 2),
        "every missing piece is wanted"
    );
    assert!(
        !diff.want.iter().any(|b| b.piece_index == 3),
        "the present file's piece is not re-fetched"
    );

    // from_wanted created the absent files so empties exist on disk.
    assert!(fetch_root.join("a.bin").exists());
    assert!(fetch_root.join("b.bin").exists());
}

/// A fetch root reached through a symlink still indexes. Android's app
/// storage is exactly this shape — `/data/user/0` is a symlink to
/// `/data/data` — and an Indexer that canonicalizes its files but not its
/// root can never strip one from the other: every phone-side fetch died
/// on "index local share" while the same code passed on a desk.
#[tokio::test]
async fn wanted_root_through_a_symlink_indexes() {
    let seed_dir = TempDir::new().expect("tempdir");
    let seed_root = seed_dir.path();
    write_file(seed_root, "a.bin", b'a', 100);
    write_file(seed_root, "b/c.bin", b'c', 50);
    let want = Indexer::from_path(seed_root)
        .await
        .expect("from_path")
        .index()
        .await
        .expect("index");

    let real_dir = TempDir::new().expect("tempdir");
    let link_dir = TempDir::new().expect("tempdir");
    let linked_root = link_dir.path().join("via-link");
    std::os::unix::fs::symlink(real_dir.path(), &linked_root).expect("symlink");

    let want_rooted = Index::new(
        linked_root.clone(),
        want.payload().clone(),
        want.files().clone(),
    );
    let have = Indexer::from_wanted(&want_rooted)
        .await
        .expect("from_wanted")
        .index()
        .await
        .expect("index through the symlink");
    // Nothing on disk yet, so nothing is had — but the index exists and
    // its files resolve under the real directory.
    assert_eq!(have.files().len(), 2);
    assert!(real_dir.path().join("a.bin").exists());
    assert!(real_dir.path().join("b/c.bin").exists());
}
