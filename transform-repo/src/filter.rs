#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use pyo3::{types::PyFunction, Python};
use regex::Regex;

use crate::builder::Builder;

pub struct TODO;

pub struct FastExportParser {}

/// A tuple of (depth, list-of-ancestors). Commits and ancestors are identified
/// by their id (their `mark` in fast-export or fast-import speak). The depth of
/// a commit is one more than the max depth of any of its ancestors.
pub struct AncestryGraph {}

pub struct ProgressWriter {}

pub struct Oid {}

pub struct RepoFilter<'py> {
    args: TODO,

    /// Repo we are exporting.
    repo_working_dir: Option<PathBuf>,

    // Convenience callbacks.
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

    /// Callbacks for acting on raw objects printed by `FastExport`.
    blob_callback: Option<&'py PyFunction>,
    commit_callback: Option<&'py PyFunction>,
    tag_callback: Option<&'py PyFunction>,
    reset_callback: Option<&'py PyFunction>,
    done_callback: Option<&'py PyFunction>,

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
    /// commits probably have suboptimal commit messages (e.g., "Merge branch
    /// feature into main").
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
    #[inline]
    pub fn builder(py: Python<'py>, args: TODO) -> Builder {
        Builder::new(py, args)
    }
}

impl<'py> From<Builder<'py>> for RepoFilter<'py> {
    fn from(b: Builder<'py>) -> Self {
        RepoFilter {
            args: b.args,
            repo_working_dir: None,
            filename_callback: b.filename_callback,
            message_callback: b.message_callback,
            name_callback: b.name_callback,
            email_callback: b.email_callback,
            refname_callback: b.refname_callback,
            blob_callback: b.blob_callback,
            commit_callback: b.commit_callback,
            tag_callback: b.tag_callback,
            reset_callback: b.reset_callback,
            done_callback: b.done_callback,
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
    use pyo3::{
        types::{PyDict, PyString},
        Python,
    };

    use crate::filter::{RepoFilter, TODO};

    #[test]
    fn parse_and_call_callback() {
        Python::with_gil(|py| {
            let mut b = RepoFilter::builder(py, TODO);
            b.filename_callback("return f\"Hello, {filename}!\"")
                .unwrap();
            let filter = b.build();
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
