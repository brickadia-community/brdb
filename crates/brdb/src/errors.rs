use std::fmt::Display;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrError {
    #[error("{0}: {1}")]
    Wrapped(String, Box<Self>),
    #[error(transparent)]
    Fs(#[from] BrFsError),
    #[error(transparent)]
    Schema(#[from] BrdbSchemaError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    World(#[from] BrdbWorldError),
    #[cfg(feature = "brdb")]
    #[error(transparent)]
    Brdb(#[from] crate::BrdbError),
    #[cfg(feature = "brz")]
    #[error(transparent)]
    Brz(#[from] crate::BrzError),
}

impl BrError {
    pub fn wrap(self, label: impl Display) -> Self {
        Self::Wrapped(label.to_string(), Box::new(self))
    }
}

// Helper trait for adding context to errors
pub trait Wrap<T> {
    fn about(self, name: impl Display) -> Result<T, BrError>;
    fn about_f(self, name: impl FnMut() -> String) -> Result<T, BrError>;
}
impl<T, E> Wrap<T> for Result<T, E>
where
    BrError: From<E>,
{
    fn about(self, name: impl Display) -> Result<T, BrError> {
        self.map_err(|e| BrError::from(e).wrap(name))
    }
    fn about_f(self, mut name: impl FnMut() -> String) -> Result<T, BrError> {
        self.map_err(|e| BrError::from(e).wrap(name()))
    }
}

#[derive(Debug, Error)]
pub enum BrFsError {
    #[error("{0}: {1}")]
    Wrapped(String, Box<Self>),
    #[cfg(feature = "brdb")]
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to decompress data: {0}")]
    Decompress(std::io::Error),
    #[error("failed to compress data: {0}")]
    Compress(std::io::Error),
    #[error("{name}: invalid size found: {found}, expected: {expected}")]
    InvalidSize {
        name: String,
        found: usize,
        expected: usize,
    },
    #[error("invalid hash found: {found:?}, expected: {expected:?}")]
    InvalidHash { found: Vec<u8>, expected: Vec<u8> },
    #[error("expected a file but found a directory at {0}")]
    ExpectedFile(String),
    #[error("file {0} does not have any content")]
    ExpectedFileContent(String),
    #[error("expected a directory but found a file {0}")]
    ExpectedDirectory(String),
    #[error("file or directory {0} does not exist")]
    NotFound(String),
    #[error("file or directory at path is not a directory: {0}")]
    NotADirectory(String),
    #[error("an absolute path is not allowed outside of the brdb root")]
    AbsolutePathNotAllowed,
    #[error("invalid fs structure: {0} vs {1}")]
    InvalidStructure(String, String),
    #[error("cannot write entry twice in same folder: {0}")]
    DuplicateName(String),
    #[error("invalid path component {0}")]
    InvalidPathComponent(String),
    #[error("missing content for {0}")]
    MissingContent(String),
}

impl BrFsError {
    pub fn wrap(self, label: impl Display) -> Self {
        Self::Wrapped(label.to_string(), Box::new(self))
    }
}

impl BrFsError {
    pub fn prepend(self, path: impl Display) -> Self {
        match self {
            BrFsError::ExpectedFile(p) => BrFsError::ExpectedFile(format!("{path}/{p}")),
            BrFsError::ExpectedDirectory(p) => BrFsError::ExpectedDirectory(format!("{path}/{p}")),
            BrFsError::NotFound(p) => BrFsError::NotFound(format!("{path}/{p}")),
            BrFsError::NotADirectory(p) => BrFsError::NotADirectory(format!("{path}/{p}")),
            other => other,
        }
    }
}

#[derive(Debug, Error)]
pub enum BrdbSchemaError {
    #[error("{0}: {1}")]
    Wrapped(String, Box<BrdbSchemaError>),
    #[error(transparent)]
    RmpValueReadError(#[from] rmp::decode::ValueReadError),
    #[error(transparent)]
    RmpValueWriteError(#[from] rmp::encode::ValueWriteError),
    #[error("error reading rmp marker: {0}")]
    RmpMarkerReadError(std::io::Error),
    #[error(transparent)]
    ReadError(#[from] std::io::Error),
    #[error(transparent)]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("schema is invalid: {0}")]
    InvalidSchema(String),
    #[error("invalid header ({0})")]
    InvalidHeader(u32),
    #[error("missing struct: {0}")]
    MissingStruct(String),
    #[error("missing struct field: {0}.{1}")]
    MissingStructField(String, String),
    #[error("missing intern {0}")]
    StringNotInterned(usize),
    #[error("unknown type: {0}")]
    UnknownType(String),
    #[error("unknown schema type: {0}")]
    UnknownSchemaType(String),
    #[error("unknown struct propery type: {0}")]
    UnknownStructPropertyType(String),
    #[error("unknown {0} asset: {1}")]
    UnknownAsset(String, usize),
    #[error("enum {enum_name} does not have a value at index {index}")]
    EnumIndexOutOfBounds { enum_name: String, index: u64 },
    #[error("index {index} out of bounds, length is {len}")]
    ArrayIndexOutOfBounds { index: usize, len: usize },
    #[error("expected type {0}, received {1}")]
    ExpectedType(String, String),
    #[error("expected array items")]
    ExpectedArrayItems,
    #[error("invalid flat type")]
    InvalidFlatType(String),
    #[error("invalid data size {1}/{2} for flat type {0}")]
    InvalidFlatDataSize(String, usize, usize),
    #[error("unsupported conversion from {0} to {1}")]
    UnimplementedCast(String, &'static str),
    #[error("failed to parse schema: {0}")]
    ParseError(String),
    #[error("unknown wire variant: {0}")]
    UnknownWireVariant(usize),
}

impl BrdbSchemaError {
    pub fn wrap(self, label: impl Display) -> Self {
        Self::Wrapped(label.to_string(), Box::new(self))
    }
}

#[derive(Debug, Error)]
pub enum BrdbWorldError {
    #[error("{0}: {1}")]
    Wrapped(String, Box<BrdbWorldError>),
    #[error("unknown brick id: {0}")]
    UnknownBrickId(usize),
    #[error("grid not in world: {0}")]
    UnknownGridId(usize),
    #[error("component name not in schema: {0}")]
    UnknownComponent(String),
    #[error(
        "component type {0} is not registered; call World::register_all_components() \
         (or register_component) before writing"
    )]
    UnregisteredComponentType(String),
    #[error("port name not in schema: {0}")]
    UnknownPort(String),
    #[error("unknown entity id: {0}")]
    UnknownEntityId(usize),
}

impl BrdbWorldError {
    pub fn wrap(self, label: impl Display) -> Self {
        Self::Wrapped(label.to_string(), Box::new(self))
    }
}
