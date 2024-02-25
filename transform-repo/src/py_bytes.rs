use std::hash::{Hash, Hasher};

use fast_export::FromBytes;
use pyo3::{types::PyBytes, Python};

#[derive(Clone, Copy, Debug)]
pub enum PyLazyBytes<'a, 'py> {
    Borrowed(&'a [u8]),
    Python(&'py PyBytes),
}

impl<'a, 'py> PyLazyBytes<'a, 'py> {
    #[inline]
    pub fn new(bytes: &'a [u8]) -> Self {
        PyLazyBytes::Borrowed(bytes)
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match *self {
            PyLazyBytes::Borrowed(bytes) => bytes,
            PyLazyBytes::Python(bytes) => bytes.as_bytes(),
        }
    }

    #[inline]
    pub fn to_python(&self, py: Python<'py>) -> &'py PyBytes {
        match *self {
            PyLazyBytes::Borrowed(bytes) => PyBytes::new(py, bytes),
            PyLazyBytes::Python(bytes) => bytes,
        }
    }
}

impl<'a> From<&'a [u8]> for PyLazyBytes<'a, 'static> {
    #[inline]
    fn from(bytes: &'a [u8]) -> Self {
        PyLazyBytes::Borrowed(bytes)
    }
}

impl<'py> From<&'py PyBytes> for PyLazyBytes<'static, 'py> {
    #[inline]
    fn from(bytes: &'py PyBytes) -> Self {
        PyLazyBytes::Python(bytes)
    }
}

impl<'a> FromBytes<'a> for PyLazyBytes<'a, 'static> {
    #[inline]
    fn from_bytes(bytes: &'a [u8]) -> Self {
        PyLazyBytes::Borrowed(bytes)
    }
}

impl PartialEq for PyLazyBytes<'_, '_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for PyLazyBytes<'_, '_> {}

impl PartialEq<[u8]> for PyLazyBytes<'_, '_> {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }
}

impl PartialEq<PyLazyBytes<'_, '_>> for [u8] {
    #[inline]
    fn eq(&self, other: &PyLazyBytes<'_, '_>) -> bool {
        self == other.as_bytes()
    }
}

impl Hash for PyLazyBytes<'_, '_> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state)
    }
}
