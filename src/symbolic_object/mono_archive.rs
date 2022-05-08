// Taken from https://github.com/getsentry/symbolic/blob/master/symbolic-debuginfo/src/shared/mono_archive.rs but without breakpad,
// due the breakpad support using a MPL-2.0 dependency, which is forbidden by bevy

use std::{fmt, iter::FusedIterator, marker::PhantomData};

use symbolic_debuginfo::{
    elf::{ElfError, ElfObject},
    pdb::{PdbError, PdbObject},
    pe::{PeError, PeObject},
    sourcebundle::{SourceBundle, SourceBundleError},
    wasm::{WasmError, WasmObject},
};

pub trait Parse<'data>: Sized {
    type Error;

    fn parse(data: &'data [u8]) -> Result<Self, Self::Error>;

    fn test(data: &'data [u8]) -> bool {
        Self::parse(data).is_ok()
    }
}

impl<'data> Parse<'data> for ElfObject<'data> {
    type Error = ElfError;

    fn test(data: &[u8]) -> bool {
        Self::test(data)
    }

    fn parse(data: &'data [u8]) -> Result<Self, ElfError> {
        Self::parse(data)
    }
}

impl<'data> Parse<'data> for PdbObject<'data> {
    type Error = PdbError;

    fn test(data: &[u8]) -> bool {
        Self::test(data)
    }

    fn parse(data: &'data [u8]) -> Result<Self, PdbError> {
        Self::parse(data)
    }
}

impl<'data> Parse<'data> for PeObject<'data> {
    type Error = PeError;

    fn test(data: &[u8]) -> bool {
        Self::test(data)
    }

    fn parse(data: &'data [u8]) -> Result<Self, PeError> {
        Self::parse(data)
    }
}

impl<'data> Parse<'data> for SourceBundle<'data> {
    type Error = SourceBundleError;

    fn parse(data: &'data [u8]) -> Result<Self, Self::Error> {
        SourceBundle::parse(data)
    }

    fn test(data: &'data [u8]) -> bool {
        SourceBundle::test(data)
    }
}

impl<'d> Parse<'d> for WasmObject<'d> {
    type Error = WasmError;

    fn test(data: &[u8]) -> bool {
        Self::test(data)
    }

    fn parse(data: &'d [u8]) -> Result<Self, WasmError> {
        Self::parse(data)
    }
}

pub struct MonoArchive<'d, P> {
    data: &'d [u8],
    _ph: PhantomData<&'d P>,
}

impl<'d, P> MonoArchive<'d, P>
where
    P: Parse<'d>,
{
    pub fn new(data: &'d [u8]) -> Self {
        MonoArchive {
            data,
            _ph: PhantomData,
        }
    }

    pub fn object(&self) -> Result<P, P::Error> {
        P::parse(self.data)
    }

    pub fn objects(&self) -> MonoArchiveObjects<'d, P> {
        // TODO(ja): Consider parsing this lazily instead.
        MonoArchiveObjects(Some(self.object()))
    }

    pub fn object_count(&self) -> usize {
        1
    }

    pub fn object_by_index(&self, index: usize) -> Result<Option<P>, P::Error> {
        match index {
            0 => self.object().map(Some),
            _ => Ok(None),
        }
    }

    #[allow(dead_code)]
    pub fn is_multi(&self) -> bool {
        false
    }
}

impl<'d, P> fmt::Debug for MonoArchive<'d, P>
where
    P: Parse<'d> + fmt::Debug,
    P::Error: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tuple = f.debug_tuple("MonoArchive");
        match self.object() {
            Ok(object) => tuple.field(&object),
            Err(error) => tuple.field(&error),
        };
        tuple.finish()
    }
}

#[derive(Debug)]
pub struct MonoArchiveObjects<'d, P>(Option<Result<P, P::Error>>)
where
    P: Parse<'d>;

impl<'d, P> Iterator for MonoArchiveObjects<'d, P>
where
    P: Parse<'d>,
{
    type Item = Result<P, P::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.take()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.0.is_some() {
            (1, Some(1))
        } else {
            (0, Some(0))
        }
    }
}

impl<'d, P> FusedIterator for MonoArchiveObjects<'d, P> where P: Parse<'d> {}
impl<'d, P> ExactSizeIterator for MonoArchiveObjects<'d, P> where P: Parse<'d> {}
