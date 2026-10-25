use std::io::{Read, Seek, Write};

use crate::{Header, PropertyTagPartial, Result, SaveGameArchive, StructType, VersionInfo};

/// Defines the type system for an archive format.
///
/// This trait allows different archive types (save games vs assets) to use
/// different representations for object references and other type-specific data.
pub trait ArchiveType: Clone + PartialEq + std::fmt::Debug + Default + serde::Serialize {
    /// The type used to represent object references in this archive format.
    /// - For save games: `String` (object path as string)
    /// - For assets: `FPackageIndex` (index into import/export tables)
    type ObjectRef: Clone
        + PartialEq
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>;

    /// Returns true if the given object reference is the null reference.
    fn is_null_object_ref(object_ref: &Self::ObjectRef) -> bool;

    /// The type used to represent soft object paths in this archive format.
    /// - For save games: `SoftObjectPath` enum with asset paths
    /// - For assets: Could be different representation
    type SoftObjectPath: Clone
        + PartialEq
        + std::fmt::Debug
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>;
}

pub trait ArchiveReader: Read + Seek {
    type ArchiveType: ArchiveType;

    fn version(&self) -> &dyn VersionInfo;

    /// Get a mutable reference to the scope for tracking the current property path.
    /// Use `scope().push(name)` and `scope().pop()` to manage the scope stack.
    fn scope(&mut self) -> &mut crate::Scope;

    /// Look up a type hint for the current scope, returning the provided default if not found.
    /// Used to disambiguate struct types in Sets and Maps.
    fn get_type_or(&mut self, default: &StructType) -> Result<StructType>;

    /// Read a string from the archive
    fn read_string(&mut self) -> Result<String>;

    /// Read a string with trailing bytes from the archive
    fn read_string_trailing(&mut self) -> Result<(String, Vec<u8>)>;

    /// Read an object reference from the archive
    fn read_object_ref(&mut self) -> Result<<Self::ArchiveType as ArchiveType>::ObjectRef>;

    /// Read a soft object path from the archive
    fn read_soft_object_path(
        &mut self,
    ) -> Result<<Self::ArchiveType as ArchiveType>::SoftObjectPath>;

    /// Record a property schema at the given path
    fn record_schema(&mut self, path: String, tag: PropertyTagPartial);

    /// Record a property schema at the current scope path
    fn record_schema_at_scope(&mut self, tag: PropertyTagPartial) {
        let path = self.path();
        self.record_schema(path, tag);
    }

    /// Get the current property path in the scope hierarchy
    fn path(&self) -> String;

    /// Returns true if diagnostic logging is enabled
    fn log(&self) -> bool {
        false
    }

    /// Returns true if errors during property parsing should produce Raw properties instead of failing.
    /// When false, parsing errors will immediately return an error.
    fn error_to_raw(&self) -> bool {
        false
    }

    /// Returns true if the property at the current scope should be skipped rather than read
    fn skip_property(&self) -> bool {
        false
    }

    /// Decode `bytes`, the byte array at the current scope, as a property list when it
    /// was asked for. `None` when it wasn't, or when the bytes don't parse as one.
    fn read_nested_properties(
        &mut self,
        _bytes: &[u8],
    ) -> Option<crate::Properties<Self::ArchiveType>> {
        None
    }
}

pub trait ArchiveWriter: Write + Seek {
    type ArchiveType: ArchiveType;

    fn version(&self) -> &dyn VersionInfo;

    /// Set version information (typically called after reading/writing the header)
    fn set_version(&mut self, header: Header);

    /// Get a mutable reference to the scope for tracking the current property path.
    /// Use `scope().push(name)` and `scope().pop()` to manage the scope stack.
    fn scope(&mut self) -> &mut crate::Scope;

    /// Write a string to the archive
    fn write_string(&mut self, string: &str) -> Result<()>;

    /// Write a string with trailing bytes to the archive
    fn write_string_trailing(&mut self, string: &str, trailing: Option<&[u8]>) -> Result<()>;

    /// Write an object reference to the archive
    fn write_object_ref(
        &mut self,
        object_ref: &<Self::ArchiveType as ArchiveType>::ObjectRef,
    ) -> Result<()>;

    /// Write a soft object path to the archive
    fn write_soft_object_path(
        &mut self,
        soft_object_path: &<Self::ArchiveType as ArchiveType>::SoftObjectPath,
    ) -> Result<()>;

    /// Get a property schema at the given path
    fn get_schema(&self, path: &str) -> Option<PropertyTagPartial>;

    /// Get the current property path in the scope hierarchy
    fn path(&self) -> String;

    /// Returns true if diagnostic logging is enabled
    fn log(&self) -> bool {
        false
    }
}

/// Archive type for save games, which use string-based object references
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct SaveGameArchiveType;

impl ArchiveType for SaveGameArchiveType {
    type ObjectRef = String;
    type SoftObjectPath = crate::SoftObjectPath;

    fn is_null_object_ref(object_ref: &Self::ObjectRef) -> bool {
        object_ref.is_empty() || object_ref == "None"
    }
}

impl<R> ArchiveReader for SaveGameArchive<R>
where
    R: Read + Seek,
{
    type ArchiveType = SaveGameArchiveType;

    fn version(&self) -> &dyn VersionInfo {
        SaveGameArchive::version(self)
    }

    fn scope(&mut self) -> &mut crate::Scope {
        &mut self.scope
    }

    fn get_type_or(&mut self, default: &StructType) -> Result<StructType> {
        SaveGameArchive::get_type_or(self, default)
    }

    fn read_string(&mut self) -> Result<String> {
        crate::read_string(self)
    }

    fn read_string_trailing(&mut self) -> Result<(String, Vec<u8>)> {
        crate::read_string_trailing(self)
    }

    fn read_object_ref(&mut self) -> Result<String> {
        crate::read_string(self)
    }

    fn read_soft_object_path(&mut self) -> Result<crate::SoftObjectPath> {
        crate::SoftObjectPath::read(self)
    }

    fn record_schema(&mut self, path: String, tag: PropertyTagPartial) {
        self.schemas.borrow_mut().record(path, tag);
    }

    fn record_schema_at_scope(&mut self, tag: PropertyTagPartial) {
        self.schemas
            .borrow_mut()
            .record_at(self.scope.as_str(), tag);
    }

    fn path(&self) -> String {
        self.scope.path()
    }

    fn log(&self) -> bool {
        SaveGameArchive::log(self)
    }

    fn error_to_raw(&self) -> bool {
        SaveGameArchive::error_to_raw(self)
    }

    fn skip_property(&self) -> bool {
        self.filter
            .as_ref()
            .is_some_and(|filter| !filter.includes(self.scope.as_str()))
    }

    fn read_nested_properties(&mut self, bytes: &[u8]) -> Option<crate::Properties> {
        // An empty array holds nothing, not even the `None` a property list ends with.
        if bytes.is_empty()
            || !self
                .nested
                .as_ref()
                .is_some_and(|nested| nested.includes(self.scope.as_str()))
        {
            return None;
        }
        // Read everything inside, with the same versions and hints, failing rather than
        // falling back to raw values: a list that doesn't parse stays as bytes.
        let mut inner = SaveGameArchive {
            stream: std::io::Cursor::new(bytes),
            version: self.version.clone(),
            types: self.types.clone(),
            scope: self.scope.clone(),
            log: self.log,
            error_to_raw: false,
            schemas: self.schemas.clone(),
            filter: None,
            nested: self.nested.clone(),
        };
        let result = crate::read_properties_until_none(&mut inner);
        let mut read = inner.stream.position();
        // `UObject::Serialize` follows the properties with a 32-bit "has object GUID"
        // flag and, when set, the GUID. Read past it; it isn't kept.
        let rest = &bytes[(read as usize).min(bytes.len())..];
        match rest {
            [0, 0, 0, 0] => read += 4,
            [1, 0, 0, 0, guid @ ..] if guid.len() == 16 => read += 20,
            _ => {}
        }
        match result {
            Ok(properties) if read == bytes.len() as u64 => Some(properties),
            Ok(_) => {
                if self.log {
                    eprintln!(
                        "\"{}\": property list ends after {read} of {} bytes, keeping bytes",
                        self.scope.as_str(),
                        bytes.len()
                    );
                }
                None
            }
            Err(e) => {
                if self.log {
                    eprintln!(
                        "\"{}\": not a property list at byte {read} ({e}), keeping bytes",
                        self.scope.as_str()
                    );
                }
                None
            }
        }
    }
}
impl<W> ArchiveWriter for SaveGameArchive<W>
where
    W: Write + Seek,
{
    type ArchiveType = SaveGameArchiveType;

    fn version(&self) -> &dyn VersionInfo {
        SaveGameArchive::version(self)
    }

    fn set_version(&mut self, header: Header) {
        SaveGameArchive::set_version(self, header)
    }

    fn scope(&mut self) -> &mut crate::Scope {
        &mut self.scope
    }

    fn write_string(&mut self, string: &str) -> Result<()> {
        crate::write_string(self, string)
    }

    fn write_string_trailing(&mut self, string: &str, trailing: Option<&[u8]>) -> Result<()> {
        crate::write_string_trailing(self, string, trailing)
    }

    fn write_object_ref(&mut self, object_ref: &String) -> Result<()> {
        crate::write_string(self, object_ref)
    }

    fn write_soft_object_path(&mut self, soft_object_path: &crate::SoftObjectPath) -> Result<()> {
        soft_object_path.write(self)
    }

    fn get_schema(&self, path: &str) -> Option<PropertyTagPartial> {
        self.schemas.borrow().get(path).cloned()
    }

    fn path(&self) -> String {
        self.scope.path()
    }

    fn log(&self) -> bool {
        SaveGameArchive::log(self)
    }
}
