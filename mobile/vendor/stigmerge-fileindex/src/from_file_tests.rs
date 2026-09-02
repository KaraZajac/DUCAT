use std::io::Write;

use hex_literal::hex;
use tempfile::NamedTempFile;

use super::*;

pub fn temp_file(pattern: u8, count: usize) -> NamedTempFile {
    let mut tempf = NamedTempFile::new().expect("temp file");
    let contents = vec![pattern; count];
    tempf.write(contents.as_slice()).expect("write temp file");
    tempf
}

#[tokio::test]
async fn single_file_index() {
    let tempf = temp_file(b'.', 1049600);

    let indexer = Indexer::from_file(tempf.path().into())
        .await
        .expect("Index::from_file");
    let index = indexer.index().await.expect("index");

    assert_eq!(
        index.root().to_owned(),
        tempf.path().canonicalize().unwrap().parent().unwrap().to_owned()
    );

    // Index files
    assert_eq!(index.files().len(), 1);
    assert_eq!(
        index.files()[0].path().to_owned(),
        tempf.path().file_name().unwrap().to_owned()
    );
    assert_eq!(
        index.files()[0].contents(),
        PayloadSlice {
            piece_offset: 0,
            starting_piece: 0,
            length: 1049600,
        }
    );

    // Index payload
    assert_eq!(
        index.payload().digest(),
        hex!("d2a89f4333b7b3c0c7b935912e4adacba55ac146f917aecf314fe6234348ecd4")
    );
    assert_eq!(index.payload().length(), 1049600);
    assert_eq!(index.payload().pieces().len(), 2);
    assert_eq!(index.payload().pieces()[0].length(), 1048576);
    assert_eq!(
        index.payload().pieces()[0].digest(),
        hex!("1bcbc7f9773ea132703529964b9c7235a4c65d86fda58a6dfa1ccbf6b29393a0")
    );
    assert_eq!(index.payload().pieces()[1].length(), 1024);
    assert_eq!(
        index.payload().pieces()[1].digest(),
        hex!("d1de06b89b7fbd85f5c35a3fc0dfcf9faae1c8639922631b7c169b15b2b3455e")
    );
}

#[tokio::test]
async fn empty_file_index() {
    let tempf = temp_file(b'.', 0);

    let indexer = Indexer::from_file(tempf.path().into())
        .await
        .expect("Index::from_file");
    let index = indexer.index().await.expect("index");

    assert_eq!(
        index.root().to_owned(),
        tempf.path().canonicalize().unwrap().parent().unwrap().to_owned()
    );

    // Index files
    assert_eq!(index.files().len(), 1);
    assert_eq!(
        index.files()[0].path().to_owned(),
        tempf.path().file_name().unwrap().to_owned()
    );
    assert_eq!(
        index.files()[0].contents(),
        PayloadSlice {
            piece_offset: 0,
            starting_piece: 0,
            length: 0,
        }
    );
    assert_eq!(index.payload().pieces().len(), 0);
    assert_eq!(
        index.payload().digest(),
        hex!("af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262")
    );
}

#[tokio::test]
async fn large_file_index() {
    let tempf = temp_file(b'.', 134217728);

    let indexer = Indexer::from_file(tempf.path().into())
        .await
        .expect("Index::from_file");
    let index = indexer.index().await.expect("index");

    assert_eq!(
        index.root().to_owned(),
        tempf.path().canonicalize().unwrap().parent().unwrap().to_owned()
    );

    // Index files
    assert_eq!(index.files().len(), 1);
    assert_eq!(
        index.files()[0].path().to_owned(),
        tempf.path().file_name().unwrap().to_owned()
    );
    assert_eq!(index.payload().pieces().len(), 128);
    assert_eq!(
        index.files()[0].contents(),
        PayloadSlice {
            piece_offset: 0,
            starting_piece: 0,
            length: 134217728,
        }
    );
}
