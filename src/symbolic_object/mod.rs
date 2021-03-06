// Taken from https://github.com/getsentry/symbolic/blob/master/symbolic-debuginfo/src/object.rs but without breakpad,
// due the breakpad support using a MPL-2.0 dependency, which is forbidden by bevy
//! Generic wrappers over various object file formats.

use std::borrow::Cow;
use std::error::Error;
use std::fmt;

use symbolic_common::{Arch, CodeId, DebugId};

use symbolic_debuginfo::dwarf::*;
use symbolic_debuginfo::elf::*;
use symbolic_debuginfo::macho::*;
use symbolic_debuginfo::pdb::*;
use symbolic_debuginfo::pe::*;
use symbolic_debuginfo::sourcebundle::*;
use symbolic_debuginfo::wasm::*;
use symbolic_debuginfo::*;
mod mono_archive;
use mono_archive::{MonoArchive, MonoArchiveObjects};

macro_rules! match_inner {
    ($value:expr, $ty:tt ($pat:pat) => $expr:expr) => {
        match $value {
            $ty::Elf($pat) => $expr,
            $ty::MachO($pat) => $expr,
            $ty::Pdb($pat) => $expr,
            $ty::Pe($pat) => $expr,
            $ty::SourceBundle($pat) => $expr,
            $ty::Wasm($pat) => $expr,
        }
    };
}

macro_rules! map_inner {
    ($value:expr, $from:tt($pat:pat) => $to:tt($expr:expr)) => {
        match $value {
            $from::Elf($pat) => $to::Elf($expr),
            $from::MachO($pat) => $to::MachO($expr),
            $from::Pdb($pat) => $to::Pdb($expr),
            $from::Pe($pat) => $to::Pe($expr),
            $from::SourceBundle($pat) => $to::SourceBundle($expr),
            $from::Wasm($pat) => $to::Wasm($expr),
        }
    };
}

macro_rules! map_result {
    ($value:expr, $from:tt($pat:pat) => $to:tt($expr:expr)) => {
        match $value {
            $from::Elf($pat) => $expr.map($to::Elf).map_err(ObjectError::transparent),
            $from::MachO($pat) => $expr.map($to::MachO).map_err(ObjectError::transparent),
            $from::Pdb($pat) => $expr.map($to::Pdb).map_err(ObjectError::transparent),
            $from::Pe($pat) => $expr.map($to::Pe).map_err(ObjectError::transparent),
            $from::SourceBundle($pat) => $expr
                .map($to::SourceBundle)
                .map_err(ObjectError::transparent),
            $from::Wasm($pat) => $expr.map($to::Wasm).map_err(ObjectError::transparent),
        }
    };
}

/// Internal representation of the object error type.
#[derive(Debug)]
enum ObjectErrorRepr {
    /// The object file format is not supported.
    UnsupportedObject,

    /// A transparent error from the inner object file type.
    Transparent(Box<dyn Error + Send + Sync + 'static>),
}

/// An error when dealing with any kind of [`Object`](enum.Object.html).
pub struct ObjectError {
    repr: ObjectErrorRepr,
}

impl ObjectError {
    /// Creates a new object error with the given representation.
    fn new(repr: ObjectErrorRepr) -> Self {
        Self { repr }
    }

    /// Creates a new object error from an arbitrary error payload.
    fn transparent<E>(source: E) -> Self
    where
        E: Into<Box<dyn Error + Send + Sync>>,
    {
        let repr = ObjectErrorRepr::Transparent(source.into());
        Self { repr }
    }
}

impl fmt::Debug for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.repr {
            ObjectErrorRepr::Transparent(ref inner) => fmt::Debug::fmt(inner, f),
            _ => fmt::Debug::fmt(&self.repr, f),
        }
    }
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.repr {
            ObjectErrorRepr::UnsupportedObject => write!(f, "unsupported object file format"),
            ObjectErrorRepr::Transparent(ref inner) => fmt::Display::fmt(inner, f),
        }
    }
}

impl Error for ObjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self.repr {
            ObjectErrorRepr::UnsupportedObject => None,
            ObjectErrorRepr::Transparent(ref inner) => inner.source(),
        }
    }
}

/// Tries to infer the object type from the start of the given buffer.
///
/// If `archive` is set to `true`, multi architecture objects will be allowed. Otherwise, only
/// single-arch objects are checked.
pub fn peek(data: &[u8], archive: bool) -> FileFormat {
    if data.len() < 16 {
        return FileFormat::Unknown;
    }

    if ElfObject::test(data) {
        FileFormat::Elf
    } else if PeObject::test(data) {
        FileFormat::Pe
    } else if PdbObject::test(data) {
        FileFormat::Pdb
    } else if SourceBundle::test(data) {
        FileFormat::SourceBundle
    } else if WasmObject::test(data) {
        FileFormat::Wasm
    } else {
        let magic = goblin::mach::parse_magic_and_ctx(data, 0).map(|(magic, _)| magic);

        match magic {
            Ok(goblin::mach::fat::FAT_MAGIC) => {
                use scroll::Pread;
                if data.pread_with::<u32>(4, scroll::BE).is_ok()
                    && archive
                    && MachArchive::test(data)
                {
                    FileFormat::MachO
                } else {
                    FileFormat::Unknown
                }
            }
            Ok(
                goblin::mach::header::MH_CIGAM_64
                | goblin::mach::header::MH_CIGAM
                | goblin::mach::header::MH_MAGIC_64
                | goblin::mach::header::MH_MAGIC,
            ) => FileFormat::MachO,
            _ => FileFormat::Unknown,
        }
    }
}

/// A generic object file providing uniform access to various file formats.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Object<'data> {
    /// Executable and Linkable Format, used on Linux.
    Elf(ElfObject<'data>),
    /// Mach Objects, used on macOS and iOS derivatives.
    MachO(MachObject<'data>),
    /// Program Database, the debug companion format on Windows.
    Pdb(PdbObject<'data>),
    /// Portable Executable, an extension of COFF used on Windows.
    Pe(PeObject<'data>),
    /// A source bundle.
    SourceBundle(SourceBundle<'data>),
    /// A WASM file.
    Wasm(WasmObject<'data>),
}

impl<'data> Object<'data> {
    /// The container format of this file, corresponding to the variant of this instance.
    pub fn file_format(&self) -> FileFormat {
        match *self {
            Object::Elf(_) => FileFormat::Elf,
            Object::MachO(_) => FileFormat::MachO,
            Object::Pdb(_) => FileFormat::Pdb,
            Object::Pe(_) => FileFormat::Pe,
            Object::SourceBundle(_) => FileFormat::SourceBundle,
            Object::Wasm(_) => FileFormat::Wasm,
        }
    }

    /// The code identifier of this object.
    ///
    /// This is a platform-dependent string of variable length that _always_ refers to the code file
    /// (e.g. executable or library), even if this object is a debug file. See the variants for the
    /// semantics of this code identifier.
    pub fn code_id(&self) -> Option<CodeId> {
        match_inner!(self, Object(ref o) => o.code_id())
    }

    /// The debug information identifier of this object.
    ///
    /// For platforms that use different identifiers for their code and debug files, this _always_
    /// refers to the debug file, regardless whether this object is a debug file or not.
    pub fn debug_id(&self) -> DebugId {
        match_inner!(self, Object(ref o) => o.debug_id())
    }

    /// The CPU architecture of this object.
    pub fn arch(&self) -> Arch {
        match_inner!(self, Object(ref o) => o.arch())
    }

    /// The kind of this object.
    pub fn kind(&self) -> ObjectKind {
        match_inner!(self, Object(ref o) => o.kind())
    }

    /// The address at which the image prefers to be loaded into memory.
    pub fn load_address(&self) -> u64 {
        match_inner!(self, Object(ref o) => o.load_address())
    }

    /// Determines whether this object exposes a public symbol table.
    pub fn has_symbols(&self) -> bool {
        match_inner!(self, Object(ref o) => o.has_symbols())
    }

    /// Returns an iterator over symbols in the public symbol table.
    pub fn symbols(&self) -> SymbolIterator<'data, '_> {
        map_inner!(self, Object(ref o) => SymbolIterator(o.symbols()))
    }

    /// Returns an ordered map of symbols in the symbol table.
    pub fn symbol_map(&self) -> SymbolMap<'data> {
        match_inner!(self, Object(ref o) => o.symbol_map())
    }

    /// Determines whether this object contains debug information.
    pub fn has_debug_info(&self) -> bool {
        match_inner!(self, Object(ref o) => o.has_debug_info())
    }

    /// Constructs a debugging session.
    ///
    /// A debugging session loads certain information from the object file and creates caches for
    /// efficient access to various records in the debug information. Since this can be quite a
    /// costly process, try to reuse the debugging session as long as possible.
    ///
    /// Objects that do not support debugging or do not contain debugging information return an
    /// empty debug session. This only returns an error if constructing the debug session fails due
    /// to invalid debug data in the object.
    ///
    /// Constructing this session will also work if the object does not contain debugging
    /// information, in which case the session will be a no-op. This can be checked via
    /// [`has_debug_info`](enum.Object.html#method.has_debug_info).
    pub fn debug_session(&self) -> Result<ObjectDebugSession<'data>, ObjectError> {
        match *self {
            Object::Elf(ref o) => o
                .debug_session()
                .map(ObjectDebugSession::Dwarf)
                .map_err(ObjectError::transparent),
            Object::MachO(ref o) => o
                .debug_session()
                .map(ObjectDebugSession::Dwarf)
                .map_err(ObjectError::transparent),
            Object::Pdb(ref o) => o
                .debug_session()
                .map(ObjectDebugSession::Pdb)
                .map_err(ObjectError::transparent),
            Object::Pe(ref o) => o
                .debug_session()
                .map(ObjectDebugSession::Pe)
                .map_err(ObjectError::transparent),
            Object::SourceBundle(ref o) => o
                .debug_session()
                .map(ObjectDebugSession::SourceBundle)
                .map_err(ObjectError::transparent),
            Object::Wasm(ref o) => o
                .debug_session()
                .map(ObjectDebugSession::Dwarf)
                .map_err(ObjectError::transparent),
        }
    }

    /// Determines whether this object contains stack unwinding information.
    pub fn has_unwind_info(&self) -> bool {
        match_inner!(self, Object(ref o) => o.has_unwind_info())
    }

    /// Determines whether this object contains embedded source
    pub fn has_sources(&self) -> bool {
        match_inner!(self, Object(ref o) => o.has_sources())
    }

    /// Determines whether this object is malformed and was only partially parsed
    pub fn is_malformed(&self) -> bool {
        match_inner!(self, Object(ref o) => o.is_malformed())
    }
}

// impl<'slf, 'data: 'slf> AsSelf<'slf> for Object<'data> {
//     type Ref = Object<'slf>;

//     fn as_self(&'slf self) -> &Self::Ref {
//         unsafe { std::mem::transmute(self) }
//     }
// }

impl<'data: 'object, 'object> ObjectLike<'data, 'object> for Object<'data> {
    type Error = ObjectError;
    type Session = ObjectDebugSession<'data>;
    type SymbolIterator = SymbolIterator<'data, 'object>;

    fn file_format(&self) -> FileFormat {
        self.file_format()
    }

    fn code_id(&self) -> Option<CodeId> {
        self.code_id()
    }

    fn debug_id(&self) -> DebugId {
        self.debug_id()
    }

    fn arch(&self) -> Arch {
        self.arch()
    }

    fn kind(&self) -> ObjectKind {
        self.kind()
    }

    fn load_address(&self) -> u64 {
        self.load_address()
    }

    fn has_symbols(&self) -> bool {
        self.has_symbols()
    }

    fn symbol_map(&self) -> SymbolMap<'data> {
        self.symbol_map()
    }

    fn symbols(&'object self) -> Self::SymbolIterator {
        self.symbols()
    }

    fn has_debug_info(&self) -> bool {
        self.has_debug_info()
    }

    fn debug_session(&self) -> Result<Self::Session, Self::Error> {
        self.debug_session()
    }

    fn has_unwind_info(&self) -> bool {
        self.has_unwind_info()
    }

    fn has_sources(&self) -> bool {
        self.has_sources()
    }

    fn is_malformed(&self) -> bool {
        self.is_malformed()
    }
}

/// A generic debugging session.
#[allow(clippy::large_enum_variant)]
#[allow(missing_docs)]
pub enum ObjectDebugSession<'d> {
    Dwarf(DwarfDebugSession<'d>),
    Pdb(PdbDebugSession<'d>),
    Pe(PeDebugSession<'d>),
    SourceBundle(SourceBundleDebugSession<'d>),
}

impl<'d> ObjectDebugSession<'d> {
    /// Returns an iterator over all functions in this debug file.
    ///
    /// Functions are iterated in the order they are declared in their compilation units. The
    /// functions yielded by this iterator include all inlinees and line records resolved.
    ///
    /// Note that the iterator holds a mutable borrow on the debug session, which allows it to use
    /// caches and optimize resources while resolving function and line information.
    pub fn functions(&self) -> ObjectFunctionIterator<'_> {
        match *self {
            ObjectDebugSession::Dwarf(ref s) => ObjectFunctionIterator::Dwarf(s.functions()),
            ObjectDebugSession::Pdb(ref s) => ObjectFunctionIterator::Pdb(s.functions()),
            ObjectDebugSession::Pe(ref s) => ObjectFunctionIterator::Pe(s.functions()),
            ObjectDebugSession::SourceBundle(ref s) => {
                ObjectFunctionIterator::SourceBundle(s.functions())
            }
        }
    }

    /// Returns an iterator over all source files referenced by this debug file.
    pub fn files(&self) -> ObjectFileIterator<'_> {
        match *self {
            ObjectDebugSession::Dwarf(ref s) => ObjectFileIterator::Dwarf(s.files()),
            ObjectDebugSession::Pdb(ref s) => ObjectFileIterator::Pdb(s.files()),
            ObjectDebugSession::Pe(ref s) => ObjectFileIterator::Pe(s.files()),
            ObjectDebugSession::SourceBundle(ref s) => ObjectFileIterator::SourceBundle(s.files()),
        }
    }

    /// Looks up a file's source contents by its full canonicalized path.
    ///
    /// The given path must be canonicalized.
    pub fn source_by_path(&self, path: &str) -> Result<Option<Cow<'_, str>>, ObjectError> {
        match *self {
            ObjectDebugSession::Dwarf(ref s) => {
                s.source_by_path(path).map_err(ObjectError::transparent)
            }
            ObjectDebugSession::Pdb(ref s) => {
                s.source_by_path(path).map_err(ObjectError::transparent)
            }
            ObjectDebugSession::Pe(ref s) => {
                s.source_by_path(path).map_err(ObjectError::transparent)
            }
            ObjectDebugSession::SourceBundle(ref s) => {
                s.source_by_path(path).map_err(ObjectError::transparent)
            }
        }
    }
}

impl<'session> DebugSession<'session> for ObjectDebugSession<'_> {
    type Error = ObjectError;
    type FunctionIterator = ObjectFunctionIterator<'session>;
    type FileIterator = ObjectFileIterator<'session>;

    fn functions(&'session self) -> Self::FunctionIterator {
        self.functions()
    }

    fn files(&'session self) -> Self::FileIterator {
        self.files()
    }

    fn source_by_path(&self, path: &str) -> Result<Option<Cow<'_, str>>, Self::Error> {
        self.source_by_path(path)
    }
}

/// An iterator over functions in an [`Object`](enum.Object.html).
#[allow(missing_docs)]
pub enum ObjectFunctionIterator<'s> {
    Dwarf(DwarfFunctionIterator<'s>),
    Pdb(PdbFunctionIterator<'s>),
    Pe(PeFunctionIterator<'s>),
    SourceBundle(SourceBundleFunctionIterator<'s>),
}

impl<'s> Iterator for ObjectFunctionIterator<'s> {
    type Item = Result<Function<'s>, ObjectError>;

    fn next(&mut self) -> Option<Self::Item> {
        match *self {
            ObjectFunctionIterator::Dwarf(ref mut i) => {
                Some(i.next()?.map_err(ObjectError::transparent))
            }
            ObjectFunctionIterator::Pdb(ref mut i) => {
                Some(i.next()?.map_err(ObjectError::transparent))
            }
            ObjectFunctionIterator::Pe(ref mut i) => {
                Some(i.next()?.map_err(ObjectError::transparent))
            }
            ObjectFunctionIterator::SourceBundle(ref mut i) => {
                Some(i.next()?.map_err(ObjectError::transparent))
            }
        }
    }
}

/// An iterator over source files in an [`Object`](enum.Object.html).
#[allow(missing_docs)]
#[allow(clippy::large_enum_variant)]
pub enum ObjectFileIterator<'s> {
    Dwarf(DwarfFileIterator<'s>),
    Pdb(PdbFileIterator<'s>),
    Pe(PeFileIterator<'s>),
    SourceBundle(SourceBundleFileIterator<'s>),
}

impl<'s> Iterator for ObjectFileIterator<'s> {
    type Item = Result<FileEntry<'s>, ObjectError>;

    fn next(&mut self) -> Option<Self::Item> {
        match *self {
            ObjectFileIterator::Dwarf(ref mut i) => {
                Some(i.next()?.map_err(ObjectError::transparent))
            }
            ObjectFileIterator::Pdb(ref mut i) => Some(i.next()?.map_err(ObjectError::transparent)),
            ObjectFileIterator::Pe(ref mut i) => Some(i.next()?.map_err(ObjectError::transparent)),
            ObjectFileIterator::SourceBundle(ref mut i) => {
                Some(i.next()?.map_err(ObjectError::transparent))
            }
        }
    }
}

/// A generic symbol iterator
#[allow(missing_docs)]
pub enum SymbolIterator<'data, 'object> {
    Elf(ElfSymbolIterator<'data, 'object>),
    MachO(MachOSymbolIterator<'data>),
    Pdb(PdbSymbolIterator<'data, 'object>),
    Pe(PeSymbolIterator<'data, 'object>),
    SourceBundle(SourceBundleSymbolIterator<'data>),
    Wasm(WasmSymbolIterator<'data, 'object>),
}

impl<'data, 'object> Iterator for SymbolIterator<'data, 'object> {
    type Item = Symbol<'data>;

    fn next(&mut self) -> Option<Self::Item> {
        match_inner!(self, SymbolIterator(ref mut iter) => iter.next())
    }
}

#[derive(Debug)]
enum ArchiveInner<'d> {
    Elf(MonoArchive<'d, ElfObject<'d>>),
    MachO(MachArchive<'d>),
    Pdb(MonoArchive<'d, PdbObject<'d>>),
    Pe(MonoArchive<'d, PeObject<'d>>),
    SourceBundle(MonoArchive<'d, SourceBundle<'d>>),
    Wasm(MonoArchive<'d, WasmObject<'d>>),
}

/// A generic archive that can contain one or more object files.
///
/// Effectively, this will only contain a single object for all file types other than `MachO`. Mach
/// objects can either be single object files or so-called _fat_ files that contain multiple objects
/// per architecture.
#[derive(Debug)]
pub struct Archive<'d>(ArchiveInner<'d>);

impl<'d> Archive<'d> {
    /// Tries to infer the object archive type from the start of the given buffer.
    pub fn peek(data: &[u8]) -> FileFormat {
        peek(data, true)
    }

    /// Tries to parse a generic archive from the given slice.
    pub fn parse(data: &'d [u8]) -> Result<Self, ObjectError> {
        let archive = match Self::peek(data) {
            FileFormat::Elf => Archive(ArchiveInner::Elf(MonoArchive::new(data))),
            FileFormat::MachO => {
                let inner = MachArchive::parse(data)
                    .map(ArchiveInner::MachO)
                    .map_err(ObjectError::transparent)?;
                Archive(inner)
            }
            FileFormat::Pdb => Archive(ArchiveInner::Pdb(MonoArchive::new(data))),
            FileFormat::Pe => Archive(ArchiveInner::Pe(MonoArchive::new(data))),
            FileFormat::SourceBundle => Archive(ArchiveInner::SourceBundle(MonoArchive::new(data))),
            FileFormat::Wasm => Archive(ArchiveInner::Wasm(MonoArchive::new(data))),
            FileFormat::Unknown | FileFormat::Breakpad => {
                return Err(ObjectError::new(ObjectErrorRepr::UnsupportedObject))
            }
        };

        Ok(archive)
    }

    /// Returns an iterator over all objects contained in this archive.
    pub fn objects(&self) -> ObjectIterator<'d, '_> {
        ObjectIterator(map_inner!(self.0, ArchiveInner(ref a) =>
            ObjectIteratorInner(a.objects())))
    }
}

#[allow(clippy::large_enum_variant)]
enum ObjectIteratorInner<'d, 'a> {
    Elf(MonoArchiveObjects<'d, ElfObject<'d>>),
    MachO(MachObjectIterator<'d, 'a>),
    Pdb(MonoArchiveObjects<'d, PdbObject<'d>>),
    Pe(MonoArchiveObjects<'d, PeObject<'d>>),
    SourceBundle(MonoArchiveObjects<'d, SourceBundle<'d>>),
    Wasm(MonoArchiveObjects<'d, WasmObject<'d>>),
}

/// An iterator over [`Object`](enum.Object.html)s in an [`Archive`](struct.Archive.html).
pub struct ObjectIterator<'d, 'a>(ObjectIteratorInner<'d, 'a>);

impl<'d, 'a> Iterator for ObjectIterator<'d, 'a> {
    type Item = Result<Object<'d>, ObjectError>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(map_result!(
            self.0,
            ObjectIteratorInner(ref mut iter) => Object(iter.next()?)
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match_inner!(self.0, ObjectIteratorInner(ref iter) => iter.size_hint())
    }
}

impl std::iter::FusedIterator for ObjectIterator<'_, '_> {}
impl ExactSizeIterator for ObjectIterator<'_, '_> {}

// TODO(ja): Implement IntoIterator for Archive
