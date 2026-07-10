use std::error::Error;

use crate::{
    BrFsReader, Brz, BrzArchiveHeader, BrzError, BrzIndexData, CompressionMethod, IntoReader,
    pending::BrPendingFs,
};

#[test]
fn test_read_write() -> Result<(), Box<dyn Error>> {
    let empty = Brz {
        index_data: BrzIndexData::default(),
        blob_data: Vec::new(),
    };

    let buf_uncompressed = empty.to_vec(None)?;
    let out_uncompressed = Brz::read_slice(&buf_uncompressed)?;
    assert_eq!(out_uncompressed.blob_data.len(), 0);
    assert_eq!(out_uncompressed.index_data.num_folders, 0);
    assert_eq!(out_uncompressed.index_data.num_files, 0);
    assert_eq!(out_uncompressed.index_data.num_blobs, 0);
    assert_eq!(out_uncompressed.index_data.folder_parent_ids.len(), 0);
    assert_eq!(out_uncompressed.index_data.folder_names.len(), 0);
    assert_eq!(out_uncompressed.index_data.file_parent_ids.len(), 0);
    assert_eq!(out_uncompressed.index_data.file_content_ids.len(), 0);
    assert_eq!(out_uncompressed.index_data.file_names.len(), 0);
    assert_eq!(out_uncompressed.index_data.compression_methods.len(), 0);
    assert_eq!(out_uncompressed.index_data.sizes_uncompressed.len(), 0);
    assert_eq!(out_uncompressed.index_data.sizes_compressed.len(), 0);
    assert_eq!(out_uncompressed.index_data.blob_hashes.len(), 0);
    assert_eq!(out_uncompressed.index_data.blob_ranges.len(), 0);
    assert_eq!(out_uncompressed.index_data.blob_total_size, 0);

    let buf_compressed = empty.to_vec(Some(14))?;
    let out_compressed = Brz::read_slice(&buf_compressed)?;
    assert_eq!(out_compressed.blob_data.len(), 0);
    assert_eq!(out_compressed.index_data.num_folders, 0);
    assert_eq!(out_compressed.index_data.num_files, 0);
    assert_eq!(out_compressed.index_data.num_blobs, 0);
    assert_eq!(out_compressed.index_data.folder_parent_ids.len(), 0);
    assert_eq!(out_compressed.index_data.folder_names.len(), 0);
    assert_eq!(out_compressed.index_data.file_parent_ids.len(), 0);
    assert_eq!(out_compressed.index_data.file_content_ids.len(), 0);
    assert_eq!(out_compressed.index_data.file_names.len(), 0);
    assert_eq!(out_compressed.index_data.compression_methods.len(), 0);
    assert_eq!(out_compressed.index_data.sizes_uncompressed.len(), 0);
    assert_eq!(out_compressed.index_data.sizes_compressed.len(), 0);
    assert_eq!(out_compressed.index_data.blob_hashes.len(), 0);
    assert_eq!(out_compressed.index_data.blob_ranges.len(), 0);
    assert_eq!(out_compressed.index_data.blob_total_size, 0);

    assert_eq!(57, buf_compressed.len()); // (the compressed value would be 62 but that's larger than the uncompressed)
    assert_eq!(57, buf_uncompressed.len());
    Ok(())
}
#[test]
fn test_archive_header() -> Result<(), Box<dyn Error>> {
    let test = |data: BrzArchiveHeader| -> Result<(), BrzError> {
        let mut vec = Vec::new();
        data.write(&mut vec)?;
        let read = BrzArchiveHeader::read(&mut vec.as_slice())?;
        assert_eq!(data.version, read.version);
        assert_eq!(data.index_method, read.index_method);
        assert_eq!(data.index_size_uncompressed, read.index_size_uncompressed);
        assert_eq!(data.index_size_compressed, read.index_size_compressed);
        assert_eq!(data.index_hash, read.index_hash);
        let mut read_vec = Vec::new();
        read.write(&mut read_vec)?;
        assert_eq!(vec, read_vec);
        Ok(())
    };

    test(BrzArchiveHeader {
        version: super::FormatVersion::Initial,
        index_method: crate::CompressionMethod::None,
        index_size_uncompressed: 0,
        index_size_compressed: 0,
        index_hash: [0; 32],
    })?;
    test(BrzArchiveHeader {
        version: super::FormatVersion::Initial,
        index_method: crate::CompressionMethod::GenericZstd,
        index_size_uncompressed: 12345,
        index_size_compressed: 12345,
        index_hash: [64; 32],
    })?;

    Ok(())
}

#[test]
fn test_index_data() -> Result<(), Box<dyn Error>> {
    let test = |data: BrzIndexData| -> Result<(), BrzError> {
        let vec = data.to_vec()?;
        let read = BrzIndexData::read(&mut vec.as_slice())?;
        assert_eq!(data.num_folders, read.num_folders);
        assert_eq!(data.num_files, read.num_files);
        assert_eq!(data.num_blobs, read.num_blobs);
        assert_eq!(data.folder_parent_ids, read.folder_parent_ids);
        assert_eq!(data.folder_names, read.folder_names);
        assert_eq!(data.file_parent_ids, read.file_parent_ids);
        assert_eq!(data.file_content_ids, read.file_content_ids);
        assert_eq!(data.file_names, read.file_names);
        assert_eq!(data.compression_methods, read.compression_methods);
        assert_eq!(data.sizes_uncompressed, read.sizes_uncompressed);
        assert_eq!(data.sizes_compressed, read.sizes_compressed);
        assert_eq!(data.blob_hashes, read.blob_hashes);
        assert_eq!(data.blob_ranges, read.blob_ranges);
        assert_eq!(data.blob_total_size, read.blob_total_size);
        let read_vec = read.to_vec()?;
        assert_eq!(vec, read_vec);
        Ok(())
    };

    test(BrzIndexData {
        num_folders: 0,
        num_files: 0,
        num_blobs: 0,
        folder_parent_ids: vec![],
        folder_names: vec![],
        file_parent_ids: vec![],
        file_content_ids: vec![],
        file_names: vec![],
        compression_methods: vec![],
        sizes_uncompressed: vec![],
        sizes_compressed: vec![],
        blob_hashes: vec![],
        blob_ranges: vec![],
        blob_total_size: 0,
    })?;
    test(BrzIndexData {
        num_folders: 1,
        num_files: 0,
        num_blobs: 0,
        folder_parent_ids: vec![-1],
        folder_names: vec!["Foo".to_string()],
        file_parent_ids: vec![],
        file_content_ids: vec![],
        file_names: vec![],
        compression_methods: vec![],
        sizes_uncompressed: vec![],
        sizes_compressed: vec![],
        blob_hashes: vec![],
        blob_ranges: vec![],
        blob_total_size: 0,
    })?;

    test(BrzIndexData {
        num_folders: 0,
        num_files: 1,
        num_blobs: 0,
        folder_parent_ids: vec![],
        folder_names: vec![],
        file_parent_ids: vec![-1],
        file_content_ids: vec![-1],
        file_names: vec!["Foo.txt".to_string()],
        compression_methods: vec![],
        sizes_uncompressed: vec![],
        sizes_compressed: vec![],
        blob_hashes: vec![],
        blob_ranges: vec![],
        blob_total_size: 0,
    })?;

    test(BrzIndexData {
        num_folders: 0,
        num_files: 0,
        num_blobs: 1,
        folder_parent_ids: vec![],
        folder_names: vec![],
        file_parent_ids: vec![],
        file_content_ids: vec![],
        file_names: vec![],
        compression_methods: vec![CompressionMethod::GenericZstd],
        sizes_uncompressed: vec![0],
        sizes_compressed: vec![0],
        blob_hashes: vec![[0; 32]],
        blob_ranges: vec![(0, 0)],
        blob_total_size: 0,
    })?;

    test(BrzIndexData {
        num_folders: 0,
        num_files: 0,
        num_blobs: 1,
        folder_parent_ids: vec![],
        folder_names: vec![],
        file_parent_ids: vec![],
        file_content_ids: vec![],
        file_names: vec![],
        compression_methods: vec![CompressionMethod::GenericZstd],
        sizes_uncompressed: vec![64],
        sizes_compressed: vec![32],
        blob_hashes: vec![[0; 32]],
        blob_ranges: vec![(0, 32)],
        blob_total_size: 32,
    })?;

    test(BrzIndexData {
        num_folders: 1,
        num_files: 1,
        num_blobs: 1,
        folder_parent_ids: vec![-1],
        folder_names: vec!["Foo".to_string()],
        file_parent_ids: vec![0],
        file_content_ids: vec![0],
        file_names: vec!["Bar.txt".to_string()],
        compression_methods: vec![CompressionMethod::None],
        sizes_uncompressed: vec![32],
        sizes_compressed: vec![0],
        blob_hashes: vec![[0; 32]],
        blob_ranges: vec![(0, 32)],
        blob_total_size: 32,
    })?;

    Ok(())
}

#[test]
fn test_pending_fs() -> Result<(), Box<dyn Error>> {
    let fs = || {
        BrPendingFs::Root(vec![
            (
                "Foo".to_string(),
                BrPendingFs::Folder(Some(vec![
                    (
                        "Bar".to_string(),
                        BrPendingFs::File(Some(b"hello".to_vec())),
                    ),
                    ("Baz".to_string(), BrPendingFs::File(Some(b"new".to_vec()))),
                    (
                        "Repeat".to_string(),
                        BrPendingFs::File(Some(b"new".to_vec())),
                    ),
                ])),
            ),
            ("Data".to_string(), BrPendingFs::File(Some(b"a".repeat(64)))),
        ])
    };

    println!("Archiving");
    let rawdata = fs().to_brz_data(None)?;
    let compdata = fs().to_brz_data(Some(14))?;
    {
        assert_eq!(rawdata.index_data.num_folders, 1);
        assert_eq!(rawdata.index_data.num_files, 4);
        assert_eq!(rawdata.index_data.num_blobs, 3);
        assert_eq!(rawdata.index_data.folder_names.len(), 1);
        assert_eq!(rawdata.index_data.file_names.len(), 4);
        assert_eq!(rawdata.index_data.folder_parent_ids.len(), 1);
        assert_eq!(rawdata.index_data.file_parent_ids.len(), 4);
        assert_eq!(rawdata.index_data.file_content_ids.len(), 4);
        assert_eq!(rawdata.index_data.compression_methods.len(), 3);
        assert_eq!(rawdata.index_data.sizes_uncompressed.len(), 3);
        assert_eq!(rawdata.index_data.sizes_compressed.len(), 3);
        assert_eq!(rawdata.index_data.blob_hashes.len(), 3);
        assert_eq!(rawdata.index_data.blob_ranges.len(), 3);
        assert_eq!(
            rawdata.index_data.blob_total_size,
            64 + b"hello".len() + b"new".len()
        );
    }

    println!("Writing");
    let raw = rawdata.to_vec(None)?;
    let comp = compdata.to_vec(Some(14))?;

    assert_eq!(raw.len(), 317);
    // 253 = compressed index (the fixed size guard actually stores the
    // zstd index now) + compressed blobs; was 270 with the index stored
    // uncompressed.
    assert_eq!(comp.len(), 253);

    println!("Reading");
    let raw = Brz::read_slice(&raw)?.into_reader();
    let comp = Brz::read_slice(&comp)?.into_reader();

    assert_eq!(raw.blob_data.len(), 72);
    assert_eq!(comp.blob_data.len(), 25);

    {
        assert_eq!(raw.index_data.num_folders, 1);
        assert_eq!(raw.index_data.num_folders, comp.index_data.num_folders);
        assert_eq!(raw.index_data.num_files, 4);
        assert_eq!(raw.index_data.num_files, comp.index_data.num_files);
        assert_eq!(raw.index_data.num_blobs, 3);
        assert_eq!(raw.index_data.num_blobs, comp.index_data.num_blobs);

        assert_eq!(raw.index_data.folder_names.len(), 1);
        assert_eq!(raw.index_data.folder_names, comp.index_data.folder_names);
        assert_eq!(raw.index_data.file_names.len(), 4);
        assert_eq!(raw.index_data.file_names, comp.index_data.file_names);
        assert_eq!(raw.index_data.folder_parent_ids.len(), 1);
        assert_eq!(
            raw.index_data.folder_parent_ids,
            comp.index_data.folder_parent_ids
        );
        assert_eq!(raw.index_data.file_parent_ids.len(), 4);
        assert_eq!(
            raw.index_data.file_parent_ids,
            comp.index_data.file_parent_ids
        );
        assert_eq!(raw.index_data.file_content_ids.len(), 4);
        assert_eq!(
            raw.index_data.file_content_ids,
            comp.index_data.file_content_ids
        );
        assert_eq!(raw.index_data.file_content_ids[0], 0);
        assert_eq!(raw.index_data.file_content_ids[1], 1);
        assert_eq!(raw.index_data.file_content_ids[2], 2);
        assert_eq!(raw.index_data.file_content_ids[3], 2);
        assert_eq!(comp.index_data.file_content_ids[0], 0);
        assert_eq!(comp.index_data.file_content_ids[1], 1);
        assert_eq!(comp.index_data.file_content_ids[2], 2);
        assert_eq!(comp.index_data.file_content_ids[3], 2);

        assert_eq!(raw.index_data.compression_methods.len(), 3);
        assert_eq!(raw.index_data.sizes_uncompressed.len(), 3);
        assert_eq!(
            raw.index_data.sizes_uncompressed,
            comp.index_data.sizes_uncompressed
        );
        assert_eq!(raw.index_data.sizes_compressed.len(), 3);
        assert_eq!(raw.index_data.blob_hashes.len(), 3);
        assert_eq!(raw.index_data.blob_hashes, comp.index_data.blob_hashes);
        assert_eq!(raw.index_data.blob_ranges.len(), 3);

        // a*64
        assert_eq!(raw.index_data.blob_ranges[0], (0, 64));
        // hello
        assert_eq!(raw.index_data.blob_ranges[1], (64, 69));
        // new
        assert_eq!(raw.index_data.blob_ranges[2], (69, 72));
        // a*64
        assert_eq!(comp.index_data.blob_ranges[0], (0, 17));
        // hello
        assert_eq!(comp.index_data.blob_ranges[1], (17, 22));
        // new
        assert_eq!(comp.index_data.blob_ranges[2], (22, 25));
    }

    // Blob data is different but we can still check the contents
    println!("Filesystem comparison");
    let rawfs = raw.get_fs()?;
    let comfs = comp.get_fs()?;
    assert_eq!(rawfs.render(), comfs.render());

    println!("Content comparison");
    let tests = [
        ("Foo/Bar", b"hello".to_vec()),
        ("Foo/Baz", b"new".to_vec()),
        ("Foo/Repeat", b"new".to_vec()),
        ("Data", b"a".repeat(64)),
    ];
    for (path, content) in tests {
        println!("Checking raw content of {path}");
        let raw_content = raw.read_file(path)?;
        assert_eq!(raw_content, content);
        println!("Checking comp content of {path}");
        let comp_content = comp.read_file(path)?;
        assert_eq!(comp_content, content);
    }

    Ok(())
}
