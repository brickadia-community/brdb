use std::fmt::Display;
use std::ops::Deref;

use crate::{errors::BrFsError, fs::BrFs, tables::BrBlob};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundFile {
    pub blob_id: i64,
    pub created_at: i64,
}

impl Deref for FoundFile {
    type Target = i64;

    fn deref(&self) -> &Self::Target {
        &self.blob_id
    }
}

impl From<FoundFile> for i64 {
    fn from(found: FoundFile) -> i64 {
        found.blob_id
    }
}

impl FoundFile {
    pub fn new(blob_id: i64, created_at: i64) -> Self {
        Self {
            blob_id,
            created_at,
        }
    }
}

pub trait BrFsReader {
    /// Find a file by its path in the brdb filesystem, returning FoundFile if found.
    fn find_file_by_path(&self, path: impl Display) -> Result<Option<FoundFile>, BrFsError> {
        let path = path.to_string();

        if path.starts_with("/") {
            return Err(BrFsError::AbsolutePathNotAllowed);
        }

        let mut components = path.split("/").peekable();
        let mut entire_path = String::from("");
        let mut parent_id = None;

        while let Some(name) = components.next() {
            entire_path.push('/');
            entire_path.push_str(name);

            // If there is more in the path, the current component must be a folder
            if components.peek().is_some() {
                let Some(next) = self
                    .find_folder(parent_id, name)
                    .map_err(|e| e.wrap(format!("find folder {entire_path}")))?
                else {
                    return Ok(None);
                };
                parent_id = Some(next);
                continue;
            }

            // Find the file in the current folder
            return self
                .find_file(parent_id, name)
                .map_err(|e| e.wrap(format!("find file {entire_path}")));
        }

        Ok(None)
    }

    /// Find a file by its path at a specific revision (timestamp), returning the
    /// `FoundFile` whose `[created_at, deleted_at)` range contains `date`.
    ///
    /// Parent folders are resolved against the current tree (folders are stable
    /// across revisions); only the leaf file is resolved at the revision.
    fn find_file_by_path_at_revision(
        &self,
        path: impl Display,
        date: i64,
    ) -> Result<Option<FoundFile>, BrFsError> {
        let path = path.to_string();

        if path.starts_with("/") {
            return Err(BrFsError::AbsolutePathNotAllowed);
        }

        let mut components = path.split("/").peekable();
        let mut entire_path = String::from("");
        let mut parent_id = None;

        while let Some(name) = components.next() {
            entire_path.push('/');
            entire_path.push_str(name);

            // If there is more in the path, the current component must be a folder
            if components.peek().is_some() {
                let Some(next) = self
                    .find_folder(parent_id, name)
                    .map_err(|e| e.wrap(format!("find folder {entire_path}")))?
                else {
                    return Ok(None);
                };
                parent_id = Some(next);
                continue;
            }

            // Find the file revision that was live at the requested date
            return self
                .find_file_at_revision(parent_id, name, date)
                .map_err(|e| e.wrap(format!("find file {entire_path}")));
        }

        Ok(None)
    }

    /// Find and read a file from the brdb filesystem, returning its decompressed content as a byte vector.
    fn read_file(&self, path: impl Display) -> Result<Vec<u8>, BrFsError> {
        let path_str = path.to_string();
        let found = self
            .find_file_by_path(&path_str)?
            .ok_or_else(|| BrFsError::NotFound(format!("file /{path_str}")))?;

        // Read the blob
        let blob = self
            .find_blob(found.blob_id)
            .map_err(|e| e.wrap(format!("find blob {}", found.blob_id)))?;
        Ok(blob
            .read()
            .map_err(|e| e.wrap(format!("read blob {}", found.blob_id)))?)
    }

    /// Find a file by its name and parent folder id in the brdb filesystem, returning its folder_id
    fn find_folder(&self, parent_id: Option<i64>, name: &str) -> Result<Option<i64>, BrFsError>;

    /// Find a file by its name and parent in the brdb filesystem, returns the FoundFile if found.
    fn find_file(&self, parent_id: Option<i64>, name: &str)
    -> Result<Option<FoundFile>, BrFsError>;

    /// Find a file by its name and parent in the brdb filesystem at a specific revision, returns the FoundFile if found.
    fn find_file_at_revision(
        &self,
        parent_id: Option<i64>,
        name: &str,
        date: i64,
    ) -> Result<Option<FoundFile>, BrFsError>;

    /// Read the metadata for a file in the brdb filesystem.
    fn find_blob(&self, content_id: i64) -> Result<BrBlob, BrFsError>;

    /// Get the filesystem representation of the BRDB database.
    fn get_fs(&self) -> Result<BrFs, BrFsError>;
}

impl<T: BrFsReader> BrFsReader for &T {
    fn find_folder(&self, parent_id: Option<i64>, name: &str) -> Result<Option<i64>, BrFsError> {
        (*self).find_folder(parent_id, name)
    }

    fn find_file(
        &self,
        parent_id: Option<i64>,
        name: &str,
    ) -> Result<Option<FoundFile>, BrFsError> {
        (*self).find_file(parent_id, name)
    }

    fn find_file_at_revision(
        &self,
        parent_id: Option<i64>,
        name: &str,
        date: i64,
    ) -> Result<Option<FoundFile>, BrFsError> {
        (*self).find_file_at_revision(parent_id, name, date)
    }

    fn find_blob(&self, content_id: i64) -> Result<BrBlob, BrFsError> {
        (*self).find_blob(content_id)
    }

    fn get_fs(&self) -> Result<BrFs, BrFsError> {
        (*self).get_fs()
    }
}

impl BrFsReader for () {
    fn find_folder(&self, _parent_id: Option<i64>, _name: &str) -> Result<Option<i64>, BrFsError> {
        unimplemented!()
    }

    fn find_file(
        &self,
        _parent_id: Option<i64>,
        _name: &str,
    ) -> Result<Option<FoundFile>, BrFsError> {
        unimplemented!()
    }

    fn find_file_at_revision(
        &self,
        _parent_id: Option<i64>,
        _name: &str,
        _date: i64,
    ) -> Result<Option<FoundFile>, BrFsError> {
        unimplemented!()
    }

    fn find_blob(&self, _content_id: i64) -> Result<BrBlob, BrFsError> {
        unimplemented!()
    }

    fn get_fs(&self) -> Result<BrFs, BrFsError> {
        unimplemented!()
    }
}
