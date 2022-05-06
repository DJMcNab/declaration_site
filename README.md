# Declaration Site

Implements iterating through the debug info associated with currently loaded
objects, to find functions. The main entry point for this crate is
[`for_some_currently_loaded_rust_functions`], which internally iterates
through all functions which can be found.

This crate was created to be used by [bevy](https://bevyengine.org/), for reporting errors
with systems. Other crates making similar use of dependency injection may
find this useful.
This can be used to get the source code location of a function item type.
This is implemented in [`declaration_by_name`] and the related functions.
However, if searching for multiple names, it will be much more efficient to
use [`for_some_currently_loaded_rust_functions`]:

```rust,no_run
# use declaration_site::{for_some_currently_loaded_rust_functions, DeclarationSite};
# use std::collections::HashMap;
let mut expected_names = HashMap::<String, Option<DeclarationSite>>::from([
    // Note that source locations for std/core are less likely to work, this is just an example
    ("std::io::read".into(), None),
    ("std::io::write".into(), None),
]);
for_some_currently_loaded_rust_functions(|name, function| {
    if let Some(result) = expected_names.get_mut(&name) {
        // Note that the `TryFrom` impl is only for `&Function`, so need to add
        // the reference
        *result = DeclarationSite::try_from(&function).ok();
    };
    // We could bail early here if all names have been filled, but that would
    // complicate the example. See the item level documentation for details
});
// Do things with `expected_names`...
```

## Caveats

This is a best-effort search only. It may fail to find a given name for any number
of reasons:

- Will not find anything on WASM.
- If the function is not linked - which may occur if it is never called. it may not be found.
- The function has been inlined
- If running on MacOS (we currently silently fail for reasons unknown, and the author cannot debug this due to not having a way to run it. Contributions welcome!)

## Changelog

See [CHANGELOG.md](CHANGELOG.md)

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license
  ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
