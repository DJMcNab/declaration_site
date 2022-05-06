#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]
use std::{env::current_exe, error::Error, fmt, fs};

use findshlibs::SharedLibrary;
use symbolic_debuginfo::Function;
use symbolic_demangle::{Demangle, DemangleOptions};

pub use findshlibs::IterationControl;

/// Attempt to get the declaration site of the function item type of the
/// given value, using its type name. This is a trivial wrapper around
/// [`declaration`], but may be easier to use since function item types
/// cannot be named.
///
/// This will (probably) return `None` for non-function item types.
///
/// See also "Caveats" in the [module level documentation](crate).
///
/// The `functions` example in this crate demonstrates this API.
pub fn declaration_of<T>(_: &T) -> Option<DeclarationSite> {
    declaration::<T>()
}

/// Attempt to get the declaration site of the function item type of the
/// given type, using its type name. In most cases, you may want to use  
/// [`declaration_of`], since function item types are unnameable.
///
/// This function uses [`declaration_by_name`] with the [type
/// name](core::any::type_name) of the type parameter `T`.
///
/// This will (probably) return `None` for non-function item types.
///
/// See also "Caveats" in the [module level documentation](crate).
///
/// Note: This could be a feature provided by [`core::any`] (using a new
/// intrinsic, returning [`&'static Location<'static>`](core::panic::Location)).
/// However, that (currently) doesn't exist. If it did, it would be:
/// - significantly faster
/// - more correct for functions as it would provide a column.
/// - support e.g. structs, unions
/// - not able to be run dynamically as [`declaration_by_name`]
pub fn declaration<T>() -> Option<DeclarationSite> {
    declaration_by_name(core::any::type_name::<T>())
}

/// Attempt to get the declaration site of a currently loaded function
/// with the given (unmangled) name.
///
/// See also "Caveats" in the [module level documentation](crate).
pub fn declaration_by_name(name: &str) -> Option<DeclarationSite> {
    let mut result = None;
    for_some_currently_loaded_rust_functions(|demangled_name, function| {
        if demangled_name == name {
            result = (&function).try_into().ok();
            IterationControl::Break
        } else {
            IterationControl::Continue
        }
    });
    result
}

/// Run `callback` on each currently loaded function which can be demangled in
/// the current context, See the caveats section on the [module level
/// documentation](crate).
///
/// As this crate depends on [`symbolic_demangle`] using the `"rust"` feature,
/// this may skip non-Rust functions (e.g. from libc), despite them having debug
/// information available. However, on platforms where mangling isn't used in
/// debug files, such as Windows' pdb files, this may also call callback with
/// non-Rust functions. Additionally, if [`symbolic_demangle`] is in the current
/// dependency tree with any other demangling features enabled supported,
/// they may also be used.
///
/// See also "Caveats" in the [module level documentation](crate).
///
/// Note that `callback` can cause this process to end early by returning [`IterationControl::Break`].
/// If doing so, return [IterationControl::Continue] to continue
///
/// Returning [`()`](unit) will be taken as returning [IterationControl::Continue].
pub fn for_some_currently_loaded_rust_functions<C>(mut callback: impl FnMut(String, Function) -> C)
where
    C: Into<IterationControl>,
{
    let mut libraries = vec![];
    // `each` might take locks - make this as short as possible to not block
    // backtraces in other threads.
    findshlibs::TargetSharedLibrary::each(|library| {
        libraries.push((
            library.name().to_owned(),
            library.debug_name().map(ToOwned::to_owned),
        ));
    });
    // Error handling:
    // In these loops, if we get an error, we just try again with the next item
    // We're not trying to be fancy here - again, this is a best effort search.
    // If nothing works, the user should have a fallback, as explained in caveats.
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
                        // We only demangle the name since `type_name` doesn't return the
                        // signature
                        function.name.demangle(DemangleOptions::name_only())
                    {
                        match callback(demangled_name, function).into() {
                            IterationControl::Break => return,
                            IterationControl::Continue => (),
                        }
                    }
                }
            }
        }
    }
}

/// A source file location, obtained from a [`symbolic_debuginfo::Function`],
/// using [`TryFrom`]/[`TryInto`].
///
/// Printing this type into a terminal will often allow it to act as a link into
/// the source code (if the working directories line up and the terminal
/// emulator supports this feature).
pub struct DeclarationSite {
    pub file: String,
    pub line: u32,
}

impl fmt::Display for DeclarationSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.file, self.line)
    }
}

/// An error returned in the [`TryFrom`] impl for [`DeclarationSite`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum DeclarationSiteError {
    /// The debug info for the function contains no source locations
    MissingLines,
}

impl fmt::Display for DeclarationSiteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeclarationSiteError::MissingLines => write!(
                f,
                "debug info contains no source locations for this function"
            ),
        }
    }
}

impl Error for DeclarationSiteError {}

/// Get the site of the first line of the function, according to the debug info.
///
/// # Errors
///
/// If the function's debug info has no source locations
impl<'a> TryFrom<&Function<'a>> for DeclarationSite {
    type Error = DeclarationSiteError;

    fn try_from(value: &Function<'a>) -> Result<Self, Self::Error> {
        let line = &value
            .lines
            .get(0)
            .ok_or(DeclarationSiteError::MissingLines)?;
        let file = line.file.path_str();

        Ok(DeclarationSite {
            file,
            line: line.line as u32,
        })
    }
}
