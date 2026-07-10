use std::io::{Read, Write};

use crate::{
    BrFsReader, BrReader, IntoReader, World, brz::reader::BrzIndex, compression::decompress,
    errors::BrError, pending::BrPendingFs, tables::BrBlob,
};

mod errors;
pub use errors::*;

mod reader;
#[cfg(test)]
mod tests;

/// As described in https://gist.github.com/Zeblote/0fc682b9df1a3e82942b613ab70d8a04

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FormatVersion {
    Initial = 0,
}

impl TryFrom<u8> for FormatVersion {
    type Error = BrzError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(FormatVersion::Initial),
            _ => Err(BrzError::InvalidFormat(value)),
        }
    }
}

/// Compression methods used in `.brz` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionMethod {
    None = 0,
    GenericZstd = 1,
}

impl TryFrom<u8> for CompressionMethod {
    type Error = BrzError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CompressionMethod::None),
            1 => Ok(CompressionMethod::GenericZstd),
            _ => Err(BrzError::InvalidCompressionMethod(value)),
        }
    }
}

pub struct BrzArchiveHeader {
    pub version: FormatVersion,
    pub index_method: CompressionMethod,
    pub index_size_uncompressed: i32,
    pub index_size_compressed: i32,
    /// A blake 3 hash of decompressed index data.
    pub index_hash: [u8; 32],
}

#[derive(Default, Clone, Debug)]
pub struct BrzIndexData {
    pub num_folders: i32,
    pub num_files: i32,
    pub num_blobs: i32,
    /// i32 * num_folders
    /// -1 if the folder is a root folder.
    pub folder_parent_ids: Vec<i32>,
    /// Folder names: (uint8 * Folder name lengths[i]) * Num folders
    /// Folder names formatted as UTF-8.
    pub folder_names: Vec<String>,
    /// i32 * num_files
    /// -1 if the file is a root file.
    pub file_parent_ids: Vec<i32>,
    /// i32 * num_files
    /// -1 if the file is empty
    pub file_content_ids: Vec<i32>,
    /// File names: (uint8 * File name lengths[i]) * Num files
    /// File names formatted as UTF-8.
    pub file_names: Vec<String>,
    // u8 * num_blobs
    pub compression_methods: Vec<CompressionMethod>,
    // i32 * num_blobs
    pub sizes_uncompressed: Vec<i32>,
    // i32 * num_blobs
    pub sizes_compressed: Vec<i32>,
    // Blob hashes: (uint8 * 32) * num_blobs
    pub blob_hashes: Vec<[u8; 32]>,
    // Binary data ranges for the blobs
    pub blob_ranges: Vec<(usize, usize)>,
    // Total size of all blobs in the archive.
    pub blob_total_size: usize,
}

#[derive(Clone)]
pub struct Brz {
    pub index_data: BrzIndexData,
    /// (Compressed) blob data as one contiguous byte array.
    pub blob_data: Vec<u8>,
}

impl IntoReader for Brz {
    type Inner = BrzIndex<Brz>;

    fn into_reader(self) -> BrReader<Self::Inner> {
        BrzIndex::new(self).into_reader()
    }
}

impl AsRef<Brz> for Brz {
    fn as_ref(&self) -> &Brz {
        self
    }
}

impl<'a> IntoReader for &'a Brz {
    type Inner = BrzIndex<&'a Brz>;

    fn into_reader(self) -> BrReader<Self::Inner> {
        BrzIndex::new(self).into_reader()
    }
}

fn read_u8(r: &mut impl Read) -> Result<u8, BrzError> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf).map_err(BrzError::IO)?;
    Ok(buf[0])
}
fn read_i32(r: &mut impl Read) -> Result<i32, BrzError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).map_err(BrzError::IO)?;
    Ok(i32::from_le_bytes(buf))
}
fn read_u16(r: &mut impl Read) -> Result<u16, BrzError> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf).map_err(BrzError::IO)?;
    Ok(u16::from_le_bytes(buf))
}
fn read_string(r: &mut impl Read, len: u16) -> Result<String, BrzError> {
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf).map_err(BrzError::IO)?;
    Ok(String::from_utf8(buf)?)
}

impl Brz {
    /// Open and read a brz archive from a file path.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Brz, BrzError> {
        Self::read(&mut std::fs::File::open(path).map_err(BrzError::IO)?)
    }

    /// Open and read a brz archive from a file path.
    pub fn new(path: impl AsRef<std::path::Path>) -> Result<Brz, BrzError> {
        Self::open(path)
    }

    /// Read a brz archive from a byte slice.
    pub fn read_slice(buf: &[u8]) -> Result<Brz, BrzError> {
        let mut cursor = std::io::Cursor::new(buf);
        Self::read(&mut cursor)
    }

    // Read a brz archive from a reader.
    pub fn read(r: &mut impl Read) -> Result<Brz, BrzError> {
        let magic = [read_u8(r)?, read_u8(r)?, read_u8(r)?];
        if magic != *b"BRZ" {
            return Err(BrzError::InvalidMagic(magic));
        }

        let header = BrzArchiveHeader::read(r)?;

        let index_buf = match header.index_method {
            CompressionMethod::None => {
                let mut buf = vec![0u8; header.index_size_uncompressed as usize];
                r.read_exact(&mut buf).map_err(BrzError::IO)?;
                buf
            }
            CompressionMethod::GenericZstd => {
                let mut compressed_buf = vec![0u8; header.index_size_compressed as usize];
                r.read_exact(&mut compressed_buf).map_err(BrzError::IO)?;
                decompress(&compressed_buf, header.index_size_uncompressed as usize)
                    .map_err(BrzError::Decompress)?
            }
        };

        // Verify the hash of the index data
        let index_hash = BrBlob::hash(&index_buf);
        if index_hash != header.index_hash {
            return Err(BrzError::InvalidIndexHash(index_hash, header.index_hash));
        }

        let index_data = BrzIndexData::read(&mut index_buf.as_slice())?;
        let mut blob_data = Vec::with_capacity(index_data.blob_total_size);
        r.read_to_end(&mut blob_data).map_err(BrzError::IO)?;

        Ok(Brz {
            index_data,
            blob_data,
        })
    }

    /// Write a brz archive to a byte vector.
    pub fn to_vec(&self, zstd_level: Option<i32>) -> Result<Vec<u8>, BrzError> {
        let mut buf = Vec::new();
        self.write(&mut buf, zstd_level)?;
        Ok(buf)
    }

    // Convert a Brz to a pending filesystem
    pub fn to_pending(&self) -> Result<BrPendingFs, BrError> {
        let reader = self.into_reader();
        Ok(reader.get_fs()?.to_pending(&*reader)?)
    }

    /// Write a pending fs to a brz file.
    pub fn write_pending(
        path: impl AsRef<std::path::Path>,
        pending: BrPendingFs,
    ) -> Result<(), BrError> {
        let mut file = std::fs::File::create(path).map_err(BrzError::IO)?;
        pending.to_brz_data(Some(14))?.write(&mut file, Some(14))?;
        Ok(())
    }

    /// Write a brz archive to a file (zstd level 14 — small but slow;
    /// see [`Brz::save_with_level`] for a faster tradeoff).
    pub fn save(path: impl AsRef<std::path::Path>, world: &World) -> Result<(), BrError> {
        Self::save_with_level(path, world, Some(14))
    }

    /// Write a brz archive to a file at an explicit zstd level.
    /// Levels 3–6 are ~5-10× faster than 14 for a few percent extra size.
    pub fn save_with_level(
        path: impl AsRef<std::path::Path>,
        world: &World,
        zstd_level: Option<i32>,
    ) -> Result<(), BrError> {
        let mut file = std::fs::File::create(path).map_err(BrzError::IO)?;
        world
            .to_unsaved()?
            .to_pending()?
            .to_brz_data(zstd_level)?
            .write(&mut file, zstd_level)?;
        Ok(())
    }

    /// Write a brz archive to a file with no compression
    pub fn save_uncompressed(
        path: impl AsRef<std::path::Path>,
        world: &World,
    ) -> Result<(), BrError> {
        Self::write_pending(path, world.to_unsaved()?.to_pending()?)
    }

    /// Write a brz archive to a writer.
    pub fn write(&self, w: &mut impl Write, zstd_level: Option<i32>) -> Result<(), BrzError> {
        w.write(b"BRZ")?;
        let mut index_data = self.index_data.to_vec()?;
        let index_size_uncompressed = index_data.len() as i32;
        let mut index_size_compressed = index_size_uncompressed;
        let mut index_method = CompressionMethod::None;
        let index_hash = BrBlob::hash(&index_data);

        if let Some(level) = zstd_level {
            let compressed_data = crate::compression::compress(&index_data, level)?;
            // Only use the compressed data if it improves file size.
            if (compressed_data.len() as i32) < index_size_uncompressed {
                index_size_compressed = compressed_data.len() as i32;
                index_method = CompressionMethod::GenericZstd;
                index_data = compressed_data;
            }
        }

        BrzArchiveHeader {
            version: FormatVersion::Initial,
            index_method,
            index_size_uncompressed,
            index_size_compressed,
            index_hash,
        }
        .write(w)?;
        w.write_all(&index_data)?;
        w.write_all(&self.blob_data)?;
        Ok(())
    }
}

impl BrzArchiveHeader {
    pub fn read(r: &mut impl Read) -> Result<BrzArchiveHeader, BrzError> {
        let version = FormatVersion::try_from(read_u8(r)?)?;
        let index_method = CompressionMethod::try_from(read_u8(r)?)?;

        let index_size_uncompressed = read_i32(r)?;
        if index_size_uncompressed < 0 {
            return Err(BrzError::InvalidIndexDecompressedLength(
                index_size_uncompressed,
            ));
        }
        let index_size_compressed = read_i32(r)?;
        if index_size_compressed < 0 {
            return Err(BrzError::InvalidIndexCompressedLength(
                index_size_compressed,
            ));
        }
        let mut index_hash = [0u8; 32];
        r.read_exact(&mut index_hash).map_err(BrzError::IO)?;

        Ok(BrzArchiveHeader {
            version,
            index_method,
            index_size_uncompressed,
            index_size_compressed,
            index_hash,
        })
    }

    pub fn write(&self, buf: &mut impl Write) -> Result<(), BrzError> {
        buf.write_all(&[self.version as u8, self.index_method as u8])?;
        buf.write_all(&self.index_size_uncompressed.to_le_bytes())?;
        buf.write_all(&self.index_size_compressed.to_le_bytes())?;
        buf.write_all(&self.index_hash)?;
        Ok(())
    }
}

impl BrzIndexData {
    pub fn read(r: &mut impl Read) -> Result<BrzIndexData, BrzError> {
        let num_folders = read_i32(r)?;
        if num_folders < 0 {
            return Err(BrzError::InvalidNumFolders(num_folders));
        }
        let num_files = read_i32(r)?;
        if num_files < 0 {
            return Err(BrzError::InvalidNumFiles(num_files));
        }
        let num_blobs = read_i32(r)?;
        if num_blobs < 0 {
            return Err(BrzError::InvalidNumBlobs(num_blobs));
        }

        let mut folder_parent_ids = Vec::with_capacity(num_folders as usize);
        for _ in 0..num_folders {
            folder_parent_ids.push(read_i32(r)?);
        }
        let folder_name_lengths = (0..num_folders)
            .map(|_| read_u16(r))
            .collect::<Result<Vec<_>, _>>()?;
        let folder_names = folder_name_lengths
            .iter()
            .map(|&len| read_string(r, len))
            .collect::<Result<Vec<_>, _>>()?;
        let mut file_parent_ids = Vec::with_capacity(num_files as usize);
        for _ in 0..num_files {
            file_parent_ids.push(read_i32(r)?);
        }
        let mut file_content_ids = Vec::with_capacity(num_files as usize);
        for _ in 0..num_files {
            file_content_ids.push(read_i32(r)?);
        }
        let file_name_lengths = (0..num_files)
            .map(|_| read_u16(r))
            .collect::<Result<Vec<_>, _>>()?;
        let file_names = file_name_lengths
            .iter()
            .map(|&len| read_string(r, len))
            .collect::<Result<Vec<_>, _>>()?;
        let mut compression_methods = Vec::with_capacity(num_blobs as usize);
        for _ in 0..num_blobs {
            compression_methods.push(CompressionMethod::try_from(read_u8(r)?)?);
        }
        let mut sizes_uncompressed = Vec::with_capacity(num_blobs as usize);
        for _ in 0..num_blobs {
            let len = read_i32(r)?;
            if len < 0 {
                return Err(BrzError::InvalidBlobDecompressedLength(len));
            }
            sizes_uncompressed.push(len);
        }
        let mut sizes_compressed = Vec::with_capacity(num_blobs as usize);
        for _ in 0..num_blobs {
            let len = read_i32(r)?;
            if len < 0 {
                return Err(BrzError::InvalidBlobCompressedLength(len));
            }
            sizes_compressed.push(len);
        }
        let mut blob_hashes = Vec::with_capacity(num_blobs as usize);
        for _ in 0..num_blobs {
            let mut hash = [0u8; 32];
            r.read_exact(&mut hash).map_err(BrzError::IO)?;
            blob_hashes.push(hash);
        }

        let mut blob_ranges = Vec::with_capacity(num_blobs as usize);
        let mut current_offset = 0;
        for i in 0..num_blobs as usize {
            let start = current_offset;
            // Index safety: `compression_methods`, `decompressed_lengths`, and `compressed_lengths`
            // are all guaranteed to have at least `num_blobs` elements.
            let length = match compression_methods[i] {
                CompressionMethod::None => sizes_uncompressed[i] as usize,
                CompressionMethod::GenericZstd => sizes_compressed[i] as usize,
            };
            let end = start + length;
            blob_ranges.push((start, end));
            current_offset = end;
        }

        Ok(BrzIndexData {
            num_folders,
            num_files,
            num_blobs,
            folder_parent_ids,
            folder_names,
            file_parent_ids,
            file_content_ids,
            file_names,
            compression_methods,
            sizes_uncompressed,
            sizes_compressed,
            blob_hashes,
            blob_ranges,
            blob_total_size: current_offset,
        })
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, BrzError> {
        let mut buf = Vec::new();
        buf.write_all(&self.num_folders.to_le_bytes())?;
        buf.write_all(&self.num_files.to_le_bytes())?;
        buf.write_all(&self.num_blobs.to_le_bytes())?;

        for &parent_id in &self.folder_parent_ids {
            buf.write_all(&parent_id.to_le_bytes())?;
        }

        // Write folder lengths and names
        for name in &self.folder_names {
            buf.write_all(&(name.len() as u16).to_le_bytes())?;
        }
        for name in &self.folder_names {
            buf.write_all(name.as_bytes())?;
        }

        for &parent_id in &self.file_parent_ids {
            buf.write_all(&parent_id.to_le_bytes())?;
        }
        for &content_id in &self.file_content_ids {
            buf.write_all(&content_id.to_le_bytes())?;
        }

        // Write file lengths and names
        for name in &self.file_names {
            buf.write_all(&(name.len() as u16).to_le_bytes())?;
        }
        for name in &self.file_names {
            buf.write_all(name.as_bytes())?;
        }

        for method in &self.compression_methods {
            buf.write_all(&[*method as u8])?;
        }
        for &size in &self.sizes_uncompressed {
            buf.write_all(&size.to_le_bytes())?;
        }
        for &size in &self.sizes_compressed {
            buf.write_all(&size.to_le_bytes())?;
        }
        for hash in &self.blob_hashes {
            buf.write_all(hash)?;
        }

        Ok(buf)
    }
}
