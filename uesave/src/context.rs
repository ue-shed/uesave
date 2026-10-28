use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{Read, Seek, Write},
    rc::Rc,
};

use serde::{Deserialize, Serialize};

use crate::{Header, PropertyTagPartial, Result, StructType};

/// Used to disambiguate types within a [`Property::Set`] or [`Property::Map`] during parsing.
#[derive(Debug, Default, Clone)]
pub struct Types {
    types: std::collections::HashMap<String, StructType>,
}
impl Types {
    /// Create an empty [`Types`] specification
    pub fn new() -> Self {
        Self::default()
    }
    /// Add a new type at the given path
    pub fn add(&mut self, path: String, t: StructType) {
        // TODO: Handle escaping of '.' in property names
        // probably should store keys as Vec<String>
        self.types.insert(path, t);
    }
}

/// Property paths to read, in the same dot-joined form as [`Types`] paths. A property
/// whose path is neither kept nor on the way to a kept path is skipped using the size
/// in its tag, without being parsed. Everything inside a kept path is read.
#[derive(Debug, Default, Clone)]
pub struct PropertyFilter {
    paths: Vec<String>,
    skipped: RefCell<Vec<crate::SkippedProperty>>,
}
impl PropertyFilter {
    /// Keep the properties at `paths` (and everything inside them)
    pub fn new(paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            paths: paths.into_iter().map(Into::into).collect(),
            skipped: RefCell::default(),
        }
    }
    /// Note that the property at `path`, `size` bytes, was skipped
    pub(crate) fn record_skipped(&self, path: &str, size: u64) {
        self.skipped.borrow_mut().push(crate::SkippedProperty {
            path: path.to_owned(),
            size,
        });
    }
    /// The properties skipped so far, leaving the list empty
    pub(crate) fn take_skipped(&self) -> Vec<crate::SkippedProperty> {
        self.skipped.take()
    }
    /// Whether the property at `path` should be read
    pub fn includes(&self, path: &str) -> bool {
        self.paths
            .iter()
            .any(|kept| is_within(path, kept) || is_within(kept, path))
    }
}

/// Paths of byte array properties that hold a serialized property list, as games do
/// when they store a struct with `UStruct::SerializeTaggedProperties` into a
/// `TArray<uint8>`. Paths match exactly, in the same dot-joined form as [`Types`].
#[derive(Debug, Default, Clone)]
pub struct NestedProperties {
    paths: std::collections::HashSet<String>,
}
impl NestedProperties {
    /// Decode the byte arrays at `paths`
    pub fn new(paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            paths: paths.into_iter().map(Into::into).collect(),
        }
    }
    /// Whether the byte array at `path` should be decoded
    pub fn includes(&self, path: &str) -> bool {
        self.paths.contains(path)
    }
}

/// Whether `path` is `scope` or a property inside it.
fn is_within(path: &str, scope: &str) -> bool {
    path.strip_prefix(scope)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Storage for property schemas (tags) separated from property data.
/// Maps property paths to their type metadata.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertySchemas {
    schemas: BTreeMap<String, PropertyTagPartial>,
}

impl PropertySchemas {
    /// Create an empty PropertySchemas
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a schema at the given path
    pub fn record(&mut self, path: String, tag: PropertyTagPartial) {
        self.schemas.insert(path, tag);
    }

    /// Record a schema at the given path, only allocating the key the first time
    /// the path is seen. Same result as [`record`](Self::record).
    pub fn record_at(&mut self, path: &str, tag: PropertyTagPartial) {
        match self.schemas.get_mut(path) {
            Some(existing) => *existing = tag,
            None => {
                self.schemas.insert(path.to_owned(), tag);
            }
        }
    }

    /// Get a schema at the given path
    pub fn get(&self, path: &str) -> Option<&PropertyTagPartial> {
        self.schemas.get(path)
    }

    /// Get all schemas
    pub fn schemas(&self) -> &BTreeMap<String, PropertyTagPartial> {
        &self.schemas
    }
}

/// Represents the current position in the property hierarchy as a stack of names.
/// Used for looking up type hints in the Types map.
///
/// The dot-joined path is kept up to date as names are pushed and popped, so
/// lookups borrow it instead of joining the components for every property.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    path: String,
    /// Length of `path` before each push, so `pop` can truncate back to it.
    starts: Vec<usize>,
}

impl Scope {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// The current path, borrowed.
    pub fn as_str(&self) -> &str {
        &self.path
    }

    pub fn push(&mut self, name: &str) {
        self.starts.push(self.path.len());
        if self.starts.len() > 1 {
            self.path.push('.');
        }
        self.path.push_str(name);
    }

    pub fn pop(&mut self) {
        if let Some(start) = self.starts.pop() {
            self.path.truncate(start);
        }
    }
}

#[derive(Debug)]
pub struct SaveGameArchive<S> {
    pub(crate) stream: S,
    pub(crate) version: Option<Header>,
    pub(crate) types: Rc<Types>,
    pub(crate) scope: Scope,
    pub(crate) log: bool,
    pub(crate) error_to_raw: bool,
    pub(crate) schemas: Rc<RefCell<PropertySchemas>>,
    pub(crate) filter: Option<Rc<PropertyFilter>>,
    pub(crate) nested: Option<Rc<NestedProperties>>,
}
impl<R: Read> Read for SaveGameArchive<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stream.read(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.stream.read_exact(buf)
    }
}
impl<S: Seek> Seek for SaveGameArchive<S> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.stream.seek(pos)
    }
}
impl<W: Write + Seek> Write for SaveGameArchive<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

impl<S> SaveGameArchive<S> {
    /// Construct a new archive around `stream`. The version (Header) must be set
    /// via [`set_version`](Self::set_version) before reading any property data,
    /// since property serialization depends on engine/package versions.
    pub fn new(stream: S) -> Self {
        SaveGameArchive {
            stream,
            version: None,
            types: Rc::new(Types::new()),
            scope: Scope::root(),
            log: false,
            error_to_raw: false,
            schemas: Rc::new(RefCell::new(PropertySchemas::new())),
            filter: None,
            nested: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn run<F, T>(stream: S, f: F) -> T
    where
        F: FnOnce(&mut SaveGameArchive<S>) -> T,
    {
        f(&mut SaveGameArchive::new(stream))
    }
    fn path(&self) -> String {
        self.scope.path()
    }
    fn get_type(&self) -> Option<&StructType> {
        self.types.types.get(self.scope.as_str())
    }
    pub fn set_version(&mut self, version: Header) {
        self.version = Some(version);
    }
    pub(crate) fn version(&self) -> &Header {
        self.version.as_ref().expect("version info not set")
    }
    pub(crate) fn log(&self) -> bool {
        self.log
    }
    pub(crate) fn error_to_raw(&self) -> bool {
        self.error_to_raw
    }
}
impl<R: Read + Seek> SaveGameArchive<R> {
    pub(crate) fn get_type_or(&mut self, t: &StructType) -> Result<StructType> {
        let offset = self.stream.stream_position()?;
        Ok(self.get_type().cloned().unwrap_or_else(|| {
            if self.log() {
                eprintln!(
                    "offset {}: StructType for \"{}\" unspecified, assuming {:?}",
                    offset,
                    self.path(),
                    t
                );
            }
            t.clone()
        }))
    }
}
