#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, Write},
};

use pyo3::types::PyFunction;

use crate::filter::{Oid, TODO};

/// A class for parsing and handling the output from fast-export. This class
/// allows the user to register callbacks when various types of data are
/// encountered in the fast-export output. The basic idea is, that
/// `FastExportParser` takes fast-export output, creates the various objects as
/// it encounters them, the user gets to use/modify these objects via callbacks,
/// and finally `FastExportParser` outputs the modified objects in fast-import
/// format (presumably so they can be used to create a new repo).
pub struct FastExportParser<'py, R: BufRead, W: Write> {
    /// A handle to the input source for the fast-export data.
    input: R,

    /// A handle to the output file for the output we generate (we call dump on
    /// many of the git elements we create).
    output: W,

    /// Callbacks for the various git elements.
    blob_callback: Option<&'py PyFunction>,
    commit_callback: Option<&'py PyFunction>,
    tag_callback: Option<&'py PyFunction>,
    reset_callback: Option<&'py PyFunction>,
    progress_callback: Option<&'py PyFunction>,
    checkpoint_callback: Option<&'py PyFunction>,
    done_callback: Option<&'py PyFunction>,

    /// Keep track of which refs appear from the export, and which make it to
    /// the import (pruning of empty commits, renaming of refs, and creating new
    /// manual objects and inserting them can cause these to differ).
    exported_refs: HashSet<Oid>,
    imported_refs: HashSet<Oid>,

    /// A list of the branches we've seen, plus the last known commit they
    /// pointed to. An entry in latest_commit/latest_orig_commit will be deleted
    /// if we get a reset for that branch. These are used because of
    /// fast-import's weird decision to allow having an implicit parent via
    /// naming the branch instead of requiring branches to be specified via
    /// `from` directives.
    latest_commit: HashMap<TODO, TODO>,
    latest_orig_commit: HashMap<TODO, TODO>,

    /// Stores the contents of the current line of input being parsed.
    current_line: Vec<u8>,
}

impl<'py, R: BufRead, W: Write> FastExportParser<'py, R, W> {
    /// Creates a new `FastExportParser`.
    ///
    /// # Differences from filter-repo
    ///
    /// The parameters here differ from filter-repo `FastExportParser.__init__`.
    /// In filter-repo, `__init__` has a different order for its callback
    /// keyword arguments, and `input` and `output` are assigned later by
    /// `FastExportParser.run`.
    pub fn new(
        input: R,
        output: W,
        blob_callback: Option<&'py PyFunction>,
        commit_callback: Option<&'py PyFunction>,
        tag_callback: Option<&'py PyFunction>,
        reset_callback: Option<&'py PyFunction>,
        progress_callback: Option<&'py PyFunction>,
        checkpoint_callback: Option<&'py PyFunction>,
        done_callback: Option<&'py PyFunction>,
    ) -> Self {
        FastExportParser {
            input,
            output,
            blob_callback,
            commit_callback,
            tag_callback,
            reset_callback,
            progress_callback,
            checkpoint_callback,
            done_callback,
            exported_refs: HashSet::new(),
            imported_refs: HashSet::new(),
            latest_commit: HashMap::new(),
            latest_orig_commit: HashMap::new(),
            current_line: Vec::new(),
        }
    }

    /// Gets the refs which were received from the export.
    #[inline]
    pub fn get_exported_refs(&self) -> &HashSet<Oid> {
        &self.exported_refs
    }

    /// Gets the refs which were sent to the import.
    #[inline]
    pub fn get_imported_refs(&self) -> &HashSet<Oid> {
        &self.imported_refs
    }
}
