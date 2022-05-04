use std::{env::current_exe, fmt, fs};

use findshlibs::SharedLibrary;
use symbolic_demangle::{Demangle, DemangleOptions};

/// A copy of [Location]
///
/// [Location]: core::panic::Location
pub struct DeclarationSite {
    file: String,
    line: u32,
    col: u32,
}
impl fmt::Display for DeclarationSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}:{}", self.file, self.line, self.col)
    }
}

/// Useful for function items
pub fn declaration_by_val<T>(_: &T) -> Option<DeclarationSite> {
    declaration::<T>()
}

pub fn declaration<T>() -> Option<DeclarationSite> {
    declaration_by_name(core::any::type_name::<T>())
}

pub fn declaration_by_name(name: &str) -> Option<DeclarationSite> {
    let mut libraries = vec![];
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
            current_exe().unwrap()
        } else {
            library_path.into()
        };
        let file_data = match fs::read(path) {
            Ok(it) => it,
            _ => continue,
        };
        let object = symbolic_debuginfo::Object::parse(&file_data).unwrap();
        let session = object.debug_session().unwrap();
        for function in session.functions() {
            if let Ok(function) = function {
                if let Some(demangled_name) = function.name.demangle(DemangleOptions::name_only()) {
                    if name == demangled_name {
                        let line = &function.lines[0];
                        let file = format!(
                            "{}/{}",
                            String::from_utf8_lossy(line.file.dir),
                            String::from_utf8_lossy(line.file.name)
                        );

                        return Some(DeclarationSite {
                            file,
                            line: line.line as u32,
                            col: 0,
                        });
                    }
                }
            }
        }
    }
    None
}
