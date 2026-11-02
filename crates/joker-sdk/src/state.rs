//! Task state (ref/04 §4.3.2, §4.12): `Shared<S>` of an async task, the
//! persisted state of `#[joker::state(persist)]`, its blob and migration,
//! and the parameter changes of `#[joker::on_params]` (§4.3.4).

use alloc::vec::Vec;
use core::cell::RefCell;

use critical_section::Mutex;

use crate::cbor::{Reader, Writer};
use crate::record::Field;
use crate::value::{PortValue, Value};

/// The state of an async task between its runs (`S: Default`). It is only
/// reached through closures, so that it never crosses an `await`.
pub struct Shared<S>(Mutex<RefCell<S>>);

impl<S> core::fmt::Debug for Shared<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Shared")
    }
}

impl<S: Default> Default for Shared<S> {
    fn default() -> Self {
        Self(Mutex::new(RefCell::new(S::default())))
    }
}

impl<S> Shared<S> {
    /// Reads the state.
    pub fn with<R>(&self, f: impl FnOnce(&S) -> R) -> R {
        critical_section::with(|cs| f(&self.0.borrow(cs).borrow()))
    }

    /// Changes the state.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut S) -> R) -> R {
        critical_section::with(|cs| f(&mut self.0.borrow(cs).borrow_mut()))
    }
}

/// A field of a persisted state: what [`Field`] encodes. Anything else is
/// `J-E0720`.
#[diagnostic::on_unimplemented(
    message = "J-E0720: `{Self}` cannot be persisted",
    label = "a field of a #[joker::state(persist)] type",
    note = "a persisted field is a record field type: a scalar, a time type, Quality, an enum or record with a joker derive, Option<T> or a bounded type"
)]
pub trait Persist: Sized {
    /// Writes the value.
    fn save(&self, w: &mut Writer<'_>);
    /// Reads a value.
    fn load(r: &mut Reader<'_>) -> Option<Self>;
}

impl<T: Field> Persist for T {
    fn save(&self, w: &mut Writer<'_>) {
        self.encode(w);
    }
    fn load(r: &mut Reader<'_>) -> Option<Self> {
        T::decode(r)
    }
}

/// What `#[joker::state]` implements for a task state.
pub trait State: Sized + Send + 'static {
    /// The version of the state schema (`version = N`, default 1).
    const VERSION: u32;
    /// The state survives the hot swap (`persist`).
    const PERSIST: bool;
    /// The fields, by name, as a CBOR map with text keys.
    fn save(&self, w: &mut Writer<'_>);
    /// The default migration: the fields with the same name, the missing
    /// ones at their initial value (ref/04 §4.12).
    fn load(blob: &StateBlob<'_>) -> Self;
}

/// The state a previous version exported for one task (ref/04 §4.12).
#[derive(Clone, Copy, Debug)]
pub struct StateBlob<'a> {
    version: u32,
    bytes: &'a [u8],
}

impl<'a> StateBlob<'a> {
    #[doc(hidden)]
    pub fn new(version: u32, bytes: &'a [u8]) -> Self {
        Self { version, bytes }
    }

    /// The state schema version of the previous version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The field `name`, if it was there with a type that decodes as `T`.
    pub fn field<T: Field>(&self, name: &str) -> Option<T> {
        self.find(name).and_then(|mut r| T::decode(&mut r))
    }

    pub(crate) fn find(&self, name: &str) -> Option<Reader<'a>> {
        let mut r = Reader::new(self.bytes);
        let n = r.map()?;
        for _ in 0..n {
            let key = r.text()?;
            if key == name {
                return Some(r);
            }
            r.skip()?;
        }
        None
    }
}

/// The parameters a new generation changes (`#[joker::on_params]`).
#[derive(Clone, Debug, Default)]
pub struct ParamChanges {
    items: Vec<(&'static str, Value)>,
}

impl ParamChanges {
    #[doc(hidden)]
    pub fn new(items: Vec<(&'static str, Value)>) -> Self {
        Self { items }
    }

    /// The changes: name and new value.
    pub fn iter(&self) -> impl Iterator<Item = (&str, Value)> + '_ {
        self.items.iter().map(|(n, v)| (*n, *v))
    }

    /// The new value of `name`, if it changes and has the type `T`.
    pub fn get<T: PortValue>(&self, name: &str) -> Option<T> {
        self.items
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, v)| v.get::<T>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blob_gives_its_fields_by_name() {
        let mut buf = [0u8; 32];
        let mut w = Writer::new(&mut buf);
        w.map(2);
        w.text("integral");
        w.float(2.5);
        w.text("count");
        w.uint(3);
        let n = w.finish().unwrap();
        let blob = StateBlob::new(1, &buf[..n]);
        assert_eq!(blob.field::<f64>("integral"), Some(2.5));
        assert_eq!(blob.field::<u64>("count"), Some(3));
        assert_eq!(blob.field::<u64>("gone"), None);
        assert_eq!(blob.field::<bool>("count"), None);
        let shared: Shared<u32> = Shared::default();
        shared.with_mut(|s| *s += 2);
        assert_eq!(shared.with(|s| *s), 2);
        let changes = ParamChanges::new(alloc::vec![("limit", Value::F64(300.0))]);
        assert_eq!(changes.get::<f64>("limit"), Some(300.0));
        assert_eq!(changes.get::<bool>("limit"), None);
    }
}
