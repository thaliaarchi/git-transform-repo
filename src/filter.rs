#![allow(dead_code, private_interfaces)]

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead, BufReader},
    path::PathBuf,
};

use pyo3::{
    types::{PyDict, PyFunction, PyList},
    PyResult, Python,
};
use regex::Regex;

struct TODO;

struct FastExportParser {}

/// A tuple of (depth, list-of-ancestors). Commits and ancestors are identified
/// by their id (their 'mark' in fast-export or fast-import speak). The depth of
/// a commit is one more than the max depth of any of its ancestors.
struct AncestryGraph {}

struct ProgressWriter {}

struct Oid {}

pub struct RepoFilter<'py> {
    args: TODO,

    /// Repo we are exporting.
    repo_working_dir: Option<PathBuf>,

    /// Callbacks for acting on objects printed by `FastExport`.
    blob_callback: Option<&'py PyFunction>,
    commit_callback: Option<&'py PyFunction>,
    tag_callback: Option<&'py PyFunction>,
    reset_callback: Option<&'py PyFunction>,
    done_callback: Option<&'py PyFunction>,

    /// Callback for acting on filenames from commits.
    filename_callback: Option<&'py PyFunction>,
    /// Callback for acting on commit and tag messages.
    message_callback: Option<&'py PyFunction>,
    /// Callback for acting on author, committer, and tagger names.
    name_callback: Option<&'py PyFunction>,
    /// Callback for acting on author, committer, and tagger names.
    email_callback: Option<&'py PyFunction>,
    /// Callback for acting on ref names from commit, tag, and reset commands.
    refname_callback: Option<&'py PyFunction>,

    input: Option<TODO>,
    /// The fast-export process.
    fe_process: Option<TODO>,
    /// Path to where the original fast-export output is stored (usually
    /// `.git/filter-repo/fast-export.original`).
    fe_orig: Option<PathBuf>,
    /// Path to where the filtered fast-export output is stored (usually
    /// `.git/filter-repo/fast-export.filtered`).
    fe_filt: Option<PathBuf>,
    /// The `FastExportParser` object we are working with.
    parser: Option<FastExportParser>,

    output: Option<TODO>,
    /// The fast-import process.
    fi_process: Option<TODO>,
    import_pipes: Option<TODO>,
    managed_output: bool,

    graph: AncestryGraph,
    /// The ancestry of commits in the original repo.
    orig_graph: AncestryGraph,

    /// Names of files that were tweaked in any commit; such paths could lead to
    /// subsequent commits being empty.
    files_tweaked: HashSet<Vec<u8>>,

    /// A set of commit hash pairs (oldhash, newhash) which used to be merge
    /// commits but due to filtering were turned into non-merge commits. The
    /// commits probably have suboptimal commit messages (e.g. "Merge branch
    /// next into master").
    commits_no_longer_merges: Vec<(Oid, Oid)>,

    /// A dict of original_ids to new_ids; filtering commits means getting new
    /// commit hash (sha1sums), and we record the mapping both for diagnostic
    /// purposes and so we can rewrite commit messages. Note that the new_id can
    /// be None rather than a commit hash if the original commit became empty
    /// and was pruned or was otherwise dropped.
    commit_renames: HashMap<TODO, TODO>,

    /// A set of original_ids for which we have not yet gotten the new_ids; we
    /// use OrderedDict because we need to know the order of insertion, but the
    /// values are always ignored (and set to None). If there was an OrderedSet
    /// class, I'd use it instead.
    pending_renames: TODO,

    /// A map from abbreviated commit hashes to the set of hashes with that
    /// prefix.
    ///
    /// It's common for commit messages to refer to commits by abbreviated
    /// commit hashes, as short as 7 characters. To facilitate translating such
    /// short hashes, we have a mapping of prefixes to full old hashes.
    commit_short_old_hashes: HashMap<[u8; 7], HashSet<Oid>>,

    /// A set of commit hash references appearing in commit messages which
    /// mapped to a valid commit that was removed entirely in the filtering
    /// process. The commit message will continue to reference the now-missing
    /// commit hash, since there was nothing to map it to.
    commits_referenced_but_removed: HashSet<TODO>,

    /// Progress handling (number of commits parsed, etc.).
    progress_writer: ProgressWriter,
    num_commits: usize,

    /// Size of blobs in the repo.
    unpacked_size: HashMap<Oid, usize>,

    /// Other vars.
    sanity_checks_handled: bool,
    finalize_handled: bool,
    orig_refs: Option<TODO>,
    new_names: HashMap<TODO, Vec<u8>>,

    /// Cached pattern.
    hash_re: Regex,
}

impl<'py> RepoFilter<'py> {
    pub fn new(
        args: TODO,
        filename_callback: Option<&'py PyFunction>,
        message_callback: Option<&'py PyFunction>,
        name_callback: Option<&'py PyFunction>,
        email_callback: Option<&'py PyFunction>,
        refname_callback: Option<&'py PyFunction>,
        blob_callback: Option<&'py PyFunction>,
        commit_callback: Option<&'py PyFunction>,
        tag_callback: Option<&'py PyFunction>,
        reset_callback: Option<&'py PyFunction>,
        done_callback: Option<&'py PyFunction>,
    ) -> Self {
        RepoFilter {
            args,
            repo_working_dir: None,
            blob_callback,
            commit_callback,
            tag_callback,
            reset_callback,
            done_callback,
            filename_callback,
            message_callback,
            name_callback,
            email_callback,
            refname_callback,
            input: None,
            fe_process: None,
            fe_orig: None,
            fe_filt: None,
            parser: None,
            output: None,
            fi_process: None,
            import_pipes: None,
            managed_output: true,
            graph: AncestryGraph::new(),
            orig_graph: AncestryGraph::new(),
            files_tweaked: HashSet::new(),
            commits_no_longer_merges: Vec::new(),
            commit_renames: HashMap::new(),
            pending_renames: TODO,
            commit_short_old_hashes: HashMap::new(),
            commits_referenced_but_removed: HashSet::new(),
            progress_writer: ProgressWriter::new(),
            num_commits: 0,
            unpacked_size: HashMap::new(),
            sanity_checks_handled: false,
            finalize_handled: false,
            orig_refs: None,
            new_names: HashMap::new(),
            hash_re: Regex::new(r"(\b[0-9a-f]{7,40}\b)").unwrap(),
        }
    }

    pub fn parse(
        py: Python<'py>,
        args: TODO,
        filename_callback: Option<&str>,
        message_callback: Option<&str>,
        name_callback: Option<&str>,
        email_callback: Option<&str>,
        refname_callback: Option<&str>,
        blob_callback: Option<&str>,
        commit_callback: Option<&str>,
        tag_callback: Option<&str>,
        reset_callback: Option<&str>,
        done_callback: Option<&str>,
    ) -> anyhow::Result<Self> {
        fn parse_callback<'py>(
            py: Python<'py>,
            name: &str,
            callback: Option<&str>,
            code_buf: &mut String,
        ) -> anyhow::Result<Option<&'py PyFunction>> {
            let Some(callback) = callback else {
                return Ok(None);
            };

            // I want to compile the callback as is, so that source positions
            // could be maintained, but it needs to be wrapped in a function,
            // because it can contain `return`. `Py_CompileString`, which both
            // `Python::eval` and `Python::run` use, does not work with `return`
            // at the top level and there seems to be no alternative API in
            // CPython.
            code_buf.clear();
            code_buf.push_str("def callback(");
            code_buf.push_str(name);
            code_buf.push_str(", _do_not_use_this_var = None):");
            match File::open(callback) {
                Ok(f) => {
                    for line in BufReader::new(f).lines() {
                        code_buf.push_str("\n  ");
                        code_buf.push_str(&line?);
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    for line in callback.lines() {
                        code_buf.push_str("\n  ");
                        code_buf.push_str(line);
                    }
                }
                Err(err) => return Err(err.into()),
            }
            code_buf.push('\n');

            // TODO: Specialize `Python::run`, so I can pass a custom filename
            // like `<commit_callback>` instead of `<string>`.

            let globals = new_py_globals(py)?;
            let locals = PyDict::new(py);
            py.run(&code_buf, Some(globals), Some(locals))?;
            let callback = locals.get_item("callback")?.unwrap();
            Ok(Some(callback.extract()?))
        }

        let mut code_buf = String::new();
        Ok(RepoFilter::new(
            args,
            parse_callback(py, "filename", filename_callback, &mut code_buf)?,
            parse_callback(py, "message", message_callback, &mut code_buf)?,
            parse_callback(py, "name", name_callback, &mut code_buf)?,
            parse_callback(py, "email", email_callback, &mut code_buf)?,
            parse_callback(py, "refname", refname_callback, &mut code_buf)?,
            parse_callback(py, "blob", blob_callback, &mut code_buf)?,
            parse_callback(py, "commit", commit_callback, &mut code_buf)?,
            parse_callback(py, "tag", tag_callback, &mut code_buf)?,
            parse_callback(py, "reset", reset_callback, &mut code_buf)?,
            parse_callback(py, "done", done_callback, &mut code_buf)?,
        ))
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

impl AncestryGraph {
    pub fn new() -> Self {
        AncestryGraph {}
    }
}

impl ProgressWriter {
    pub fn new() -> Self {
        ProgressWriter {}
    }
}

#[cfg(test)]
mod tests {
    use pyo3::types::PyString;

    use super::*;

    #[test]
    fn parse_and_call_callback() {
        Python::with_gil(|py| {
            let filter = RepoFilter::parse(
                py,
                TODO,
                Some("return f\"Hello, {filename}!\""),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let res = filter
                .filename_callback
                .unwrap()
                .call(("world", PyDict::new(py)), None)
                .unwrap();
            let s = res.downcast::<PyString>().unwrap().to_str().unwrap();
            assert_eq!(s, "Hello, world!");
        });
    }
}
