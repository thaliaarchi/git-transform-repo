use std::{
    fmt::{self, Debug, Formatter},
    fs::File,
    io::{self, BufRead, BufReader},
};

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
        #[doc = $doc_name]
        /// to a function body string. If the string is a filename, it will be
        /// read from that file.
        #[inline]
        pub fn [<$name _callback>](&mut self, callback: &str) -> anyhow::Result<&mut Self> {
            self.[<$name _callback>] = Some(self.parse_callback(stringify!($name), callback)?);
            Ok(self)
        }

        /// Sets the Python callback for processing
        #[doc = $doc_name]
        /// to a function, which has already been parsed.
        #[inline]
        pub fn [<$name _callback_object>](&mut self, callback: &'py PyFunction) -> &mut Self {
            self.[<$name _callback>] = Some(callback);
            self
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

    fn parse_callback(&mut self, name: &str, callback: &str) -> anyhow::Result<&'py PyFunction> {
        // I want to compile the callback as is, so that source positions could
        // be maintained, but it needs to be wrapped in a function, because it
        // can contain `return`. `Py_CompileString`, which both `Python::eval`
        // and `Python::run` use, does not work with `return` at the top level
        // and there seems to be no alternative API in CPython.
        self.code_buf.clear();
        self.code_buf.push_str("def callback(");
        self.code_buf.push_str(name);
        self.code_buf.push_str(", _do_not_use_this_var = None):");
        match File::open(callback) {
            Ok(f) => {
                for line in BufReader::new(f).lines() {
                    self.code_buf.push_str("\n  ");
                    self.code_buf.push_str(&line?);
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                for line in callback.lines() {
                    self.code_buf.push_str("\n  ");
                    self.code_buf.push_str(line);
                }
            }
            Err(err) => return Err(err.into()),
        }
        self.code_buf.push('\n');

        // TODO: Specialize `Python::run`, so I can pass a custom filename
        // like `<commit_callback>` instead of `<string>`.

        let globals = new_py_globals(self.py)?;
        let locals = PyDict::new(self.py);
        self.py.run(&self.code_buf, Some(globals), Some(locals))?;
        let callback = locals.get_item("callback")?.unwrap();
        Ok(callback.extract()?)
    }
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
