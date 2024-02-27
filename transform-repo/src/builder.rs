// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of git-transform-repo, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    fmt::{self, Debug, Formatter},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use anyhow::{bail, Result};
use paste::paste;
use pyo3::{
    types::{PyDict, PyFunction, PyList},
    PyResult, Python,
};

use crate::filter::{RepoFilter, TODO};

/// A builder for constructing a [`RepoFilter`].
pub struct Builder<'py> {
    py: Python<'py>,
    pub(crate) args: TODO,
    pub(crate) filename_callback: Option<&'py PyFunction>,
    pub(crate) message_callback: Option<&'py PyFunction>,
    pub(crate) name_callback: Option<&'py PyFunction>,
    pub(crate) email_callback: Option<&'py PyFunction>,
    pub(crate) refname_callback: Option<&'py PyFunction>,
    pub(crate) blob_callback: Option<&'py PyFunction>,
    pub(crate) commit_callback: Option<&'py PyFunction>,
    pub(crate) tag_callback: Option<&'py PyFunction>,
    pub(crate) reset_callback: Option<&'py PyFunction>,
    pub(crate) done_callback: Option<&'py PyFunction>,
    code_buf: String,
}

macro_rules! callback(($name:ident, $doc_name:literal) => {
    paste! {
        /// Sets the Python callback for processing
        #[doc = concat!($doc_name, ".")]
        /// It may be a function body from a `&str`, `&Path`, or `&mut BufRead`,
        /// or an already parsed `&'py PyFunction`.
        #[inline]
        pub fn [<$name _callback>]<T: ToCallback<'py>>(&mut self, callback: T) -> Result<&mut Self> {
            if self.[<$name _callback>].is_some() {
                bail!("{} callback redefined", stringify!($name));
            }
            self.code_buf.clear();
            let callback = callback.to_callback(self.py, stringify!($name), &mut self.code_buf)?;
            self.[<$name _callback>] = Some(callback);
            Ok(self)
        }
    }
});

impl<'py> Builder<'py> {
    /// Creates a new `RepoFilter` builder with no callbacks.
    #[inline]
    pub fn new(py: Python<'py>, args: TODO) -> Self {
        Builder {
            py,
            args,
            filename_callback: None,
            message_callback: None,
            name_callback: None,
            email_callback: None,
            refname_callback: None,
            blob_callback: None,
            commit_callback: None,
            tag_callback: None,
            reset_callback: None,
            done_callback: None,
            code_buf: String::new(),
        }
    }

    /// Builds a `RepoFilter` with the current configuration.
    #[inline]
    pub fn build(self) -> RepoFilter<'py> {
        self.into()
    }

    callback!(filename, "filenames");
    callback!(message, "messages (both commit messages and tag messages)");
    callback!(name, "names of people");
    callback!(email, "email addresses");
    callback!(refname, "refnames");
    callback!(blob, "blob objects");
    callback!(commit, "commit objects");
    callback!(tag, "tag objects");
    callback!(reset, "reset objects");
    callback!(done, "the end of the stream");
}

impl Clone for Builder<'_> {
    #[inline]
    fn clone(&self) -> Self {
        Builder {
            py: self.py,
            args: TODO,
            filename_callback: self.filename_callback,
            message_callback: self.message_callback,
            name_callback: self.name_callback,
            email_callback: self.email_callback,
            refname_callback: self.refname_callback,
            blob_callback: self.blob_callback,
            commit_callback: self.commit_callback,
            tag_callback: self.tag_callback,
            reset_callback: self.reset_callback,
            done_callback: self.done_callback,
            code_buf: String::new(),
        }
    }
}

impl Debug for Builder<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Builder")
            .field("filename_callback", &self.filename_callback)
            .field("message_callback", &self.message_callback)
            .field("name_callback", &self.name_callback)
            .field("email_callback", &self.email_callback)
            .field("refname_callback", &self.refname_callback)
            .field("blob_callback", &self.blob_callback)
            .field("commit_callback", &self.commit_callback)
            .field("tag_callback", &self.tag_callback)
            .field("reset_callback", &self.reset_callback)
            .field("done_callback", &self.done_callback)
            .finish()
    }
}

/// A type that can be converted to a Python callback.
///
/// This allows for overloading of the callback setters in `Builder`.
pub trait ToCallback<'py> {
    fn to_callback(
        self,
        py: Python<'py>,
        name: &str,
        code_buf: &mut String,
    ) -> Result<&'py PyFunction>;
}

impl<'py> ToCallback<'py> for &str {
    #[inline]
    fn to_callback(
        self,
        py: Python<'py>,
        name: &str,
        code_buf: &mut String,
    ) -> Result<&'py PyFunction> {
        parse_callback(py, &mut self.as_bytes(), name, code_buf)
    }
}

impl<'py> ToCallback<'py> for &Path {
    #[inline]
    fn to_callback(
        self,
        py: Python<'py>,
        name: &str,
        code_buf: &mut String,
    ) -> Result<&'py PyFunction> {
        let mut f = BufReader::new(File::open(self)?);
        parse_callback(py, &mut f, name, code_buf)
    }
}

impl<'py, T: BufRead> ToCallback<'py> for &mut T {
    #[inline]
    fn to_callback(
        self,
        py: Python<'py>,
        name: &str,
        code_buf: &mut String,
    ) -> Result<&'py PyFunction> {
        parse_callback(py, self, name, code_buf)
    }
}

impl<'py> ToCallback<'py> for &'py PyFunction {
    #[inline]
    fn to_callback(
        self,
        _py: Python<'py>,
        _name: &str,
        _buf: &mut String,
    ) -> Result<&'py PyFunction> {
        Ok(self)
    }
}

fn parse_callback<'py>(
    py: Python<'py>,
    callback: &mut dyn BufRead,
    name: &str,
    code_buf: &mut String,
) -> Result<&'py PyFunction> {
    // Since callbacks can contain `return`, they need to be wrapped in a
    // function. Otherwise, I could invoke `Py_CompileString` without
    // `PyEval_EvalCode` and keep the same source positions for error messages.
    // If they were changed incompatibly from filter-repo to use setters instead
    // of `return`, this would be possible.
    code_buf.push_str("def callback(");
    code_buf.push_str(name);
    code_buf.push_str(", _do_not_use_this_var = None):");
    for line in callback.lines() {
        code_buf.push_str("\n  ");
        code_buf.push_str(&line?);
    }
    code_buf.push('\n');

    // TODO: Specialize `Python::run`, so I can pass a custom filename like
    // `<commit_callback>` instead of `<string>`.
    let globals = new_py_globals(py)?;
    let locals = PyDict::new(py);
    py.run(code_buf, Some(globals), Some(locals))?;
    let callback = locals.get_item("callback")?.unwrap();
    Ok(callback.extract()?)
}

fn new_py_globals<'py>(py: Python<'py>) -> PyResult<&'py PyDict> {
    // git-filter-repo uses `globals()`, which leaks many internal details. It
    // was probably only intended to expose imports and the public API
    // (`__all__`).
    //
    // TODO: Recreate the public library API in Rust and expose it to callbacks.

    let globals = PyDict::new(py);

    for import in [
        "argparse",
        "collections",
        "fnmatch",
        "gettext",
        "io",
        "os",
        "platform",
        "re",
        "shutil",
        "subprocess",
        "sys",
        "time",
        "textwrap",
    ] {
        globals.set_item(import, py.import(import)?)?;
    }
    let datetime = py.import("datetime")?;
    globals.set_item("tzinfo", datetime.getattr("tzinfo")?)?;
    globals.set_item("timedelta", datetime.getattr("timedelta")?)?;
    globals.set_item("datetime", datetime.getattr("datetime")?)?;

    globals.set_item(
        "__all__",
        PyList::new(
            py,
            [
                "Blob",
                "Reset",
                "FileChange",
                "Commit",
                "Tag",
                "Progress",
                "Checkpoint",
                "FastExportParser",
                "ProgressWriter",
                "string_to_date",
                "date_to_string",
                "record_id_rename",
                "GitUtils",
                "FilteringOptions",
                "RepoFilter",
            ],
        ),
    )?;
    Ok(globals)
}
