//! ## Caveats
//!
//! This is a best-effort search only
//!

use std::{env::current_exe, fmt, fs};

use findshlibs::SharedLibrary;
use symbolic_debuginfo::Function;
use symbolic_demangle::{Demangle, DemangleOptions};

/// Attempt to get the declaration site of the given function item type
///
/// This will (likely) return `None` for
pub fn declaration_of<T>(_: &T) -> Option<DeclarationSite> {
    declaration::<T>()
}

pub fn declaration<T>() -> Option<DeclarationSite> {
    declaration_by_name(core::any::type_name::<T>())
}

pub fn declaration_by_name(name: &str) -> Option<DeclarationSite> {
    for_each_currently_loaded_rust_function(|demangled_name, function| {
        (demangled_name == name)
            .then(|| (&function).try_into().ok())
            .flatten()
    })
}

/// Run `callback` on each currently loaded function which can be demangled in the current context,
/// See the caveats section on the [module level documentation](crate).
///
/// As this crate depends on [`symbolic_demangle`] using the `"rust"` feature, this may skip
/// non-Rust functions which have debug information available.
/// However, on platforms where mangling isn't used in debug files, such as Windows' pdb files,
/// this may also call callback with non-Rust functions. Additionally, if [`symbolic_demangle`] is
/// in your tree with any other demanglings supported, they may also be used.
pub fn for_each_currently_loaded_rust_function<R>(
    mut callback: impl FnMut(String, Function) -> Option<R>,
) -> Option<R> {
    let mut libraries = vec![];
    // Each might take locks - make this as short as possible to not block backtraces in other threads.
    findshlibs::TargetSharedLibrary::each(|library| {
        libraries.push((
            library.name().to_owned(),
            library.debug_name().map(ToOwned::to_owned),
        ));
    });
    for (library_path, debug_path) in libraries {
        let path = if let Some(debug_path) = debug_path {
            debug_path.into()
        } else if library_path.len() == 0 {
            match current_exe() {
                Ok(it) => it,
                Err(_) => continue,
            }
        } else {
            library_path.into()
        };
        let file_data = match fs::read(path) {
            Ok(it) => it,
            _ => continue,
        };
        let archive = match symbolic_debuginfo::Archive::parse(&file_data) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for object in archive.objects() {
            let object = match object {
                Ok(it) => it,
                Err(_) => continue,
            };
            let session = match object.debug_session() {
                Ok(it) => it,
                Err(_) => continue,
            };
            for function in session.functions() {
                if let Ok(function) = function {
                    if let Some(demangled_name) =
                        function.name.demangle(DemangleOptions::name_only())
                    {
                        match callback(demangled_name, function) {
                            ret @ Some(_) => return ret,
                            None => (),
                        }
                    }
                }
            }
        }
    }
    None
}

/// A source file location, obtained from a [`symbolic_debuginfo::Function`], using
/// [`FunctionExt::declaration_site`].
///
/// A type which can be used to
pub struct DeclarationSite {
    pub file: String,
    pub line: u32,
}

impl fmt::Display for DeclarationSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.file, self.line)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct MissingLines;

impl<'a> TryFrom<&Function<'a>> for DeclarationSite {
    type Error = MissingLines;

    fn try_from(value: &Function<'a>) -> Result<Self, Self::Error> {
        let line = &value.lines.get(0).ok_or(MissingLines)?;
        let file = line.file.path_str();

        Ok(DeclarationSite {
            file,
            line: line.line as u32,
        })
    }
}
