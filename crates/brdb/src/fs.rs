use indexmap::IndexMap;

use crate::{
    BrFsReader,
    errors::BrFsError,
    pending::BrPendingFs,
    tables::{BrBlob, BrFile, BrFolder},
};

#[derive(Debug, Clone)]
pub enum BrFs {
    Root(IndexMap<String, BrFs>),
    Folder(BrFolder, IndexMap<String, BrFs>),
    File(BrFile),
}

#[cfg(feature = "brdb")]
pub(crate) fn now() -> i64 {
    // web-time works on wasm (std::time panics there); re-exports std off-wasm
    #[cfg(feature = "wasm")]
    use web_time::{SystemTime, UNIX_EPOCH};
    #[cfg(not(feature = "wasm"))]
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now();
    now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

impl BrFs {
    #[cfg(feature = "brdb")]
    pub fn write_pending(
        &self,
        description: &str,
        db: &crate::Brdb,
        pending: BrPendingFs,
        zstd_level: Option<i32>,
    ) -> Result<(), crate::BrdbError> {
        let created_at = now();
        let tx = db.conn.unchecked_transaction()?;
        // Create the revision
        db.create_revision(&description, created_at)
            .map_err(|e| e.wrap("Create Revision"))?;
        // Write the pending changes
        self.write_pending_internal(db, pending, created_at, zstd_level)?;
        // Commit the transaction (errors will result in rollback)
        tx.commit()?;
        Ok(())
    }

    #[cfg(feature = "brdb")]
    fn write_pending_internal(
        &self,
        db: &crate::Brdb,
        pending: BrPendingFs,
        created_at: i64,
        zstd_level: Option<i32>,
    ) -> Result<(), crate::BrdbError> {
        let (parent, children, changes) = match (self, pending) {
            // Empty folder is noop
            (BrFs::Folder(_, _), BrPendingFs::Folder(None)) => return Ok(()),
            // Empty file is noop
            (BrFs::File(_), BrPendingFs::File(None)) => return Ok(()),
            // Directory handling
            (BrFs::Root(children), BrPendingFs::Root(files)) => (None, children, files),
            (BrFs::Folder(folder, children), BrPendingFs::Folder(Some(files))) => {
                (Some(folder.folder_id), children, files)
            }
            // Existing file handling
            (BrFs::File(file), BrPendingFs::File(Some(content))) => {
                let hash = BrBlob::hash(&content);

                // Check if this blob already exists and the content is the same
                if let Some(blob) = db.find_blob_by_hash(content.len(), &hash)?
                    && file.content_id == Some(blob.blob_id)
                {
                    return Ok(());
                }
                // Delete the old file (because the content id changed)
                db.delete_file(file.file_id, created_at)?;

                // Insert the blob
                let content_id = db.insert_blob(content, hash, zstd_level)?;
                // Insert the file, reusing the old one's parent_id
                db.insert_file(&file.name, file.parent_id, content_id, created_at)?;
                return Ok(());
            }
            (l, r) => return Err(BrFsError::InvalidStructure(l.render(), r.to_string()).into()),
        };

        let mut seen = std::collections::HashSet::new();

        for (name, change) in changes {
            if seen.contains(&name) {
                return Err(BrFsError::DuplicateName(name.clone()).into());
            }
            seen.insert(name.clone());

            // If the child exists, update/replace it
            if let Some(child) = children.get(&name) {
                child
                    .write_pending_internal(db, change, created_at, zstd_level)
                    .map_err(|e| e.wrap(name))?;
                continue;
            }

            Self::insert_pending(db, &name, parent, change, created_at, zstd_level)
                .map_err(|e| e.wrap(name))?;
        }

        // Queue up all children that were not visited by the changes.
        let mut queue = children
            .iter()
            .filter_map(|(name, child)| (!seen.contains(name)).then_some(child))
            .collect::<std::collections::VecDeque<_>>();

        // All descendants of non-visited children must be deleted.
        while let Some(child) = queue.pop_front() {
            match child {
                BrFs::Root(children) => {
                    for (_, child) in children {
                        queue.push_back(child);
                    }
                }
                BrFs::Folder(folder, children) => {
                    db.delete_folder(folder.folder_id, created_at)
                        .map_err(|e| e.wrap(format!("Delete Folder {}", folder.name)))?;
                    for (_, child) in children {
                        queue.push_back(child);
                    }
                }
                BrFs::File(file) => {
                    db.delete_file(file.file_id, created_at)
                        .map_err(|e| e.wrap(format!("Delete File {}", file.name)))?;
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "brdb")]
    /// Insert a pending filesystem entry into the database without any
    /// existing structure.
    fn insert_pending(
        db: &crate::Brdb,
        name: &str,
        parent: Option<i64>,
        pending: BrPendingFs,
        created_at: i64,
        zstd_level: Option<i32>,
    ) -> Result<(), crate::BrdbError> {
        match pending {
            BrPendingFs::Root(files) => {
                return Err(BrFsError::InvalidStructure(
                    "root".to_string(),
                    BrPendingFs::Root(files).to_string(),
                )
                .into());
            }
            // Empty folder is a noop
            BrPendingFs::Folder(None) => {}
            // Emtpy file is a noop
            BrPendingFs::File(None) => {}
            BrPendingFs::Folder(Some(items)) => {
                // Create this folder, then insert its children
                let folder_id = db.insert_folder(&name, parent, now())?;
                for (name, child) in items {
                    // Recursively insert the child
                    Self::insert_pending(db, &name, Some(folder_id), child, created_at, zstd_level)
                        .map_err(|e| e.wrap(name))?;
                }
            }
            BrPendingFs::File(Some(content)) => {
                let hash = BrBlob::hash(&content);
                // Check if this blob already exists
                let content_id = if let Some(blob) = db.find_blob_by_hash(content.len(), &hash)? {
                    // If the blob already exists, reuse it
                    blob.blob_id
                } else {
                    // Insert the blob
                    db.insert_blob(content, hash, zstd_level)
                        .map_err(|e| e.wrap("Blob"))?
                };

                // Insert the file
                db.insert_file(&name, parent, content_id, created_at)?;
            }
        }
        Ok(())
    }

    pub fn is_root(&self) -> bool {
        matches!(self, BrFs::Root(_))
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, BrFs::Folder(_, _))
    }

    pub fn is_file(&self) -> bool {
        matches!(self, BrFs::File(_))
    }

    /// Convert this filesystem to a pending filesystem with all files present
    pub fn to_pending(&self, reader: &impl BrFsReader) -> Result<BrPendingFs, BrFsError> {
        Self::to_pending_internal(&self, Some(reader))
    }

    /// Convert this filesystem to a pending filesystem all files in Patch mode (None for unchanged)
    pub fn to_pending_patch(&self) -> Result<BrPendingFs, BrFsError> {
        Self::to_pending_internal(&self, None::<&()>)
    }

    /// Convert this filesystem to a pending filesystem
    fn to_pending_internal(
        &self,
        reader: Option<&impl BrFsReader>,
    ) -> Result<BrPendingFs, BrFsError> {
        Ok(match self {
            BrFs::Root(children) => BrPendingFs::Root(
                children
                    .iter()
                    .map(|(name, child)| Ok((name.to_owned(), child.to_pending_internal(reader)?)))
                    .collect::<Result<Vec<(String, BrPendingFs)>, BrFsError>>()?,
            ),
            BrFs::Folder(_folder, children) => BrPendingFs::Folder(Some(
                children
                    .iter()
                    .map(|(name, child)| Ok((name.to_owned(), child.to_pending_internal(reader)?)))
                    .collect::<Result<Vec<(String, BrPendingFs)>, BrFsError>>()?,
            )),
            BrFs::File(f) => BrPendingFs::File(reader.map(|r| f.read(r)).transpose()?),
        })
    }

    /// Navigate a brdb filesystem to a specific path.
    pub fn cd(&self, path: impl AsRef<str>) -> Result<BrFs, BrFsError> {
        if !self.is_root() && path.as_ref().starts_with("/") {
            return Err(BrFsError::AbsolutePathNotAllowed);
        }

        let mut components = path
            .as_ref()
            .split('/')
            .filter(|s| !s.is_empty())
            .peekable();
        let mut curr = self;

        while let Some(name) = components.next() {
            match curr {
                BrFs::Root(children) | BrFs::Folder(_, children) => {
                    if let Some(child) = children.get(name) {
                        curr = child;
                    } else {
                        return Err(BrFsError::NotFound(format!("{}/{name}", curr.name())));
                    }
                }
                // Cannot cd into a file
                BrFs::File(_) if components.peek().is_some() => {
                    return Err(BrFsError::ExpectedDirectory(curr.name()));
                }
                // Ensure the file name matches if it's the last component
                BrFs::File(f) if f.name == name => {
                    return Ok(curr.clone());
                }
                BrFs::File(_) => {
                    return Err(BrFsError::NotFound(format!("{}/{name}", curr.name())));
                }
            }
        }

        Ok(curr.clone())
    }

    /// Read the content of a file in the brdb filesystem.
    pub fn read_blob(&self, db: &impl BrFsReader) -> Result<BrBlob, BrFsError> {
        let BrFs::File(file) = self else {
            return Err(BrFsError::ExpectedFile(self.name().into()));
        };
        let Some(content_id) = file.content_id else {
            return Err(BrFsError::ExpectedFileContent(file.name.as_str().into()));
        };
        db.find_blob(content_id)
    }

    pub fn read(&self, db: &impl BrFsReader) -> Result<Vec<u8>, BrFsError> {
        let BrFs::File(file) = self else {
            return Err(BrFsError::ExpectedFile(self.name().into()));
        };
        file.read(db)
    }

    pub fn name(&self) -> String {
        match self {
            BrFs::Root(_) => "".to_string(),
            BrFs::Folder(folder, _) => folder.name.clone(),
            BrFs::File(file) => file.name.clone(),
        }
    }

    pub fn for_each(&self, func: &mut impl FnMut(&BrFs)) {
        func(self);
        match self {
            // Invoke for_each for each of the entries in each folder
            BrFs::Root(dir) | BrFs::Folder(_, dir) => {
                for fs in dir.values() {
                    fs.for_each(func)
                }
            }
            BrFs::File(_) => {}
        }
    }

    pub fn filter_map_file<T>(&self, mut func: impl FnMut(&BrFile) -> Option<T>) -> Vec<T> {
        let mut res = vec![];
        self.for_each(&mut |fs| match fs {
            BrFs::File(file) => {
                if let Some(r) = func(file) {
                    res.push(r);
                }
            }
            _ => {}
        });
        res
    }

    pub fn render(&self) -> String {
        self.render_inner(0)
    }

    fn render_inner(&self, depth: usize) -> String {
        let pad = "   |".repeat(depth);
        match self {
            BrFs::Root(children) => {
                let mut output = String::new();
                for child in children.values() {
                    output.push_str(&child.render_inner(depth + 1));
                }
                output
            }
            BrFs::Folder(dir, children) => {
                let mut output = String::new();
                output.push_str(&format!("{pad}-- {}/\n", dir.name));
                for child in children.values() {
                    output.push_str(&child.render_inner(depth + 1));
                }
                output
            }
            BrFs::File(brdb_file) => {
                let file_path = if depth == 0 {
                    brdb_file.name.clone()
                } else {
                    format!("{pad}-- {}", brdb_file.name)
                };
                format!("{file_path}\n")
            }
        }
    }
}

impl BrFile {
    /// Read (and decompress) the content of a blob in the brdb filesystem.
    pub fn read(&self, db: &impl BrFsReader) -> Result<Vec<u8>, BrFsError> {
        let Some(content_id) = self.content_id else {
            return Err(BrFsError::ExpectedFileContent(self.name.as_str().into()).into());
        };
        db.find_blob(content_id)?.read()
    }
}
