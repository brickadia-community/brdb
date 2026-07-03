use std::{collections::HashMap, path::Path};

use indexmap::IndexMap;
use rusqlite::{Connection, params};

use crate::{
    BrFsReader, IntoReader, World,
    compression::compress,
    errors::{BrError, BrFsError},
    fs::{BrFs, now},
    pending::BrPendingFs,
    tables::{BrBlob, BrFile, BrFolder},
};

mod errors;
pub use errors::*;
mod reader;

pub struct Brdb {
    pub conn: Connection,
}

pub const REQUIRED_TABLES: [&str; 4] = ["blobs", "revisions", "folders", "files"];
pub const BRDB_SQLITE_SCHEMA: &str = include_str!("./brdb.sql");

impl Brdb {
    /// Open a new in-memory BRDB database.
    pub fn new_memory() -> Result<Self, BrdbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(BRDB_SQLITE_SCHEMA)?;
        let db = Self { conn };
        db.ensure_tables_exist()?;
        db.create_revision("Initial Revision", now())?;
        Ok(db)
    }

    /// Create a new BRDB database at the specified path.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BrdbError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(BRDB_SQLITE_SCHEMA)?;
        let db = Self { conn };
        db.ensure_tables_exist()?;
        db.create_revision("Initial Revision", now())?;
        Ok(db)
    }

    /// Open an existing BRDB database at the specified path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BrdbError> {
        let db = Self {
            conn: Connection::open(path)?,
        };
        db.ensure_tables_exist()?;
        Ok(db)
    }

    /// Open an existing BRDB database at the specified path in read-only mode.
    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Self, BrdbError> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let db = Self { conn };
        db.ensure_tables_exist()?;
        Ok(db)
    }

    /// Create or open a BRDB database at the specified path.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, BrdbError> {
        let path = path.as_ref();
        if path.exists() {
            Self::open(path)
        } else {
            Self::create(path)
        }
    }

    /// Write a pending operation to the BRDB filesystem.
    pub fn write_pending(
        &self,
        description: impl AsRef<str>,
        pending: BrPendingFs,
    ) -> Result<(), BrError> {
        let fs = self.tree(None, 0)?;
        fs.write_pending(description.as_ref(), self, pending, Some(14))?;
        Ok(())
    }

    // Convert a Brdb to a filled out pending filesystem
    pub fn to_pending(&self) -> Result<BrPendingFs, BrError> {
        let reader = self.into_reader();
        Ok(reader.get_fs()?.to_pending(&*reader)?)
    }

    // Convert a Brdb to a partial pending filesystem, for patching
    // specific files inside
    pub fn to_pending_patch(&self) -> Result<BrPendingFs, BrError> {
        let reader = self.into_reader();
        Ok(reader.get_fs()?.to_pending(&*reader)?)
    }

    /// Save a world to the BRDB database.
    pub fn save(&self, description: impl AsRef<str>, world: &World) -> Result<(), BrError> {
        self.write_pending(description.as_ref(), world.to_unsaved()?.to_pending()?)?;
        Ok(())
    }

    /// Ensure that all required tables exist in the database.
    fn ensure_tables_exist(&self) -> Result<(), BrdbError> {
        for t in &REQUIRED_TABLES {
            if !self.conn.table_exists(None, *t)? {
                return Err(BrdbError::MissingTable(t));
            }
        }
        Ok(())
    }

    /// Obtain the SQLite schema of the BRDB database as a string.
    pub fn sqlite_schema(&self) -> Result<String, BrdbError> {
        let schemas = self
            .conn
            .prepare("SELECT sql FROM sqlite_schema")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(format!("{}", schemas.join("\n")))
    }

    /// Insert a new folder into the database.
    pub fn insert_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        created_at: i64,
    ) -> Result<i64, BrdbError> {
        self.conn.execute(
            "INSERT INTO folders (name, parent_id, created_at)
            VALUES (?1, ?2, ?3);",
            params![name, parent_id, created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a new file into the database, linking it to a content blob.
    pub fn insert_file(
        &self,
        name: &str,
        parent_id: Option<i64>,
        content_id: i64,
        created_at: i64,
    ) -> Result<i64, BrdbError> {
        self.conn.execute(
            "INSERT INTO files (name, parent_id, content_id, created_at)
            VALUES (?1, ?2, ?3, ?4);",
            params![name, parent_id, content_id, created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a new blob into the database, compressing it if a zstd level is specified.
    pub fn insert_blob(
        &self,
        mut content: Vec<u8>,
        hash: [u8; 32],
        zstd_level: Option<i32>,
    ) -> Result<i64, BrdbError> {
        if let Some(existing) = self.find_blob_by_hash(content.len(), &hash)? {
            return Ok(existing.blob_id);
        }
        let size_uncompressed = content.len() as i64;
        let mut size_compressed = size_uncompressed;
        let mut compression = 0;

        // Compress the content if a zstd level is specified
        if let Some(zstd_level) = zstd_level {
            let compressed = compress(&content, zstd_level).map_err(BrFsError::Compress)?;
            if (compressed.len() as i64) < size_uncompressed {
                size_compressed = compressed.len() as i64;
                compression = 1;
                content = compressed;
            }
        }

        Ok(self.insert_blob_row(BrBlob {
            blob_id: -1,
            compression,
            size_uncompressed,
            size_compressed,
            delta_base_id: None,
            hash: hash.to_vec(),
            content,
        })?)
    }

    /// Insert a new blob into the database, ignoring the id
    pub fn insert_blob_row(&self, blob: BrBlob) -> Result<i64, BrdbError> {
        self.conn.execute(
            "INSERT INTO blobs (compression, size_uncompressed, size_compressed, delta_base_id, hash, content)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![
                blob.compression,
                blob.size_uncompressed,
                blob.size_compressed,
                blob.delta_base_id,
                blob.hash,
                blob.content
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Check if a blob with the given hash exists in the database.
    pub fn find_blob_by_hash(&self, size: usize, hash: &[u8]) -> Result<Option<BrBlob>, BrdbError> {
        let res = self.conn
            .query_one(
                "SELECT blob_id, compression, size_uncompressed, size_compressed, delta_base_id, hash, content
                FROM blobs
                WHERE hash = ?1 AND size_uncompressed = ?2
                LIMIT 1;",
                params![hash, size],
                |row| {
                    Ok(BrBlob {
                        blob_id: row.get(0)?,
                        compression: row.get(1)?,
                        size_uncompressed: row.get(2)?,
                        size_compressed: row.get(3)?,
                        delta_base_id: row.get(4)?,
                        hash: row.get(5)?,
                        content: row.get(6)?,
                    })
                },
            );
        match res {
            Ok(blob) => Ok(Some(blob)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(BrdbError::Sqlite(e)),
        }
    }

    /// Create a new revision in the database with the given description and timestamp.
    pub fn create_revision(&self, description: &str, created_at: i64) -> Result<i64, BrdbError> {
        self.conn.execute(
            "INSERT INTO revisions (description, created_at)
            VALUES (?1, ?2);",
            params![description, created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Mark a file as deleted by setting its `deleted_at` timestamp.
    pub fn delete_file(&self, file_id: i64, deleted_at: i64) -> Result<(), BrdbError> {
        self.conn.execute(
            "UPDATE files SET deleted_at = ?2 WHERE file_id = ?1;",
            params![file_id, deleted_at],
        )?;
        Ok(())
    }

    /// Mark a folder as deleted by setting its `deleted_at` timestamp.
    pub fn delete_folder(&self, folder_id: i64, deleted_at: i64) -> Result<(), BrdbError> {
        self.conn.execute(
            "UPDATE folders SET deleted_at = ?2 WHERE folder_id = ?1;",
            params![folder_id, deleted_at],
        )?;
        Ok(())
    }

    fn tree(&self, parent: Option<BrFolder>, depth: usize) -> Result<BrFs, BrFsError> {
        let (parent_query, params) = if let Some(p) = parent.as_ref() {
            ("= ?1", params![p.folder_id])
        } else {
            ("IS NULL", params![])
        };
        let dirs = self
            .conn
            .prepare(&format!(
                "SELECT name, folder_id, parent_id, created_at, deleted_at
                FROM folders
                WHERE parent_id {parent_query} AND deleted_at IS NULL
                ORDER BY name;"
            ))?
            .query_map(params, |row| {
                Ok(BrFolder {
                    name: row.get(0)?,
                    folder_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    created_at: row.get(3)?,
                    deleted_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut children = IndexMap::new();

        for dir in dirs {
            children.insert(dir.name.clone(), self.tree(Some(dir), depth + 1)?);
        }

        if let Some(parent) = parent.as_ref() {
            let files = self
                .conn
                .prepare(
                    "SELECT name, file_id, parent_id, content_id, created_at, deleted_at
                    FROM files
                    WHERE parent_id = ?1 AND deleted_at IS NULL
                    ORDER BY name;",
                )?
                .query_map(params![parent.folder_id], |row| {
                    let name: String = row.get(0)?;
                    Ok((
                        name.clone(),
                        BrFs::File(BrFile {
                            name,
                            file_id: row.get(1)?,
                            parent_id: row.get(2)?,
                            content_id: row.get(3)?,
                            created_at: row.get(4)?,
                            deleted_at: row.get(5)?,
                        }),
                    ))
                })?
                .collect::<Result<HashMap<_, _>, _>>()?;
            children.extend(files);
        }

        Ok(match parent {
            Some(p) => BrFs::Folder(p, children),
            None => BrFs::Root(children),
        })
    }
}
