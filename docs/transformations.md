# Advanced repo transformations

The operations in filter-repo are mostly subtractive, which makes any additive
operations more difficult.

## Interleaving repos

Splicing repos together is useful for several different kinds of
transformations.

### Use case: Disjoint files (monorepo)

This feature is needed when splicing multiple repos together, such as when
joining repos into a monorepo. Each input sub-repo is first filtered to move its
files to a subdirectory, then their histories are interleaved to produce a
unified history. Files wouldn't conflict between the input repos, but it comes
with all the caveats mentioned in related [filter-repo commits](./filter-repo.md#interleaving-repos).

I think [git-subrepo](https://github.com/ingydotnet/git-subrepo/tree/master)
uses this approach.

### Use case: Overlapping files (releases)

Repos that are constructed for the same project, but which have different ranges
of releases, would be merged differently. Whenever the spliced history switches
between repos, a `deleteall` directive would be issued, so that spliced-in
changes do not bleed into successive commits.

This is already handled with replace refs and vanilla filter-repo, but an
automatic mechanism to setup those parent replacements would be useful.

I could use this in numerous ways: Releases of Inferno could be unified into a
history. Quine Relay's [spoiler branch](./git.md#use-case-quine-relay) of
force-pushed single commits could be turned into a history. And more.

## Splitting repos

A transformation to split a repo into repos for subdirectories would be
convenient. It is already possible with separate invocations of filter-repo for
each split repo, but first-class support would allow for hashes in messages
to be rewritten when they reference hashes now moved to a split-off repo. Plus,
it would be more efficient in one pass.

## Converting submodules and subtrees

Converting between submodules and subtrees is useful, both extract subtrees to
submodules and incorporating submodules as subtrees.

When open-sourcing a component, a team may want to switch to using a submodule
for its subtree. If the subtree was replaced with a submodule through the whole
history, it would have seamless integration. This is a special case of
splitting, except the out-of-tree commits for submodules would need to made
before the submodule head is bumped.

The converse, moving a submodule in-tree is a special case of splicing repos.
The files are disjoint and the merge points are defined, so many problems,
except for the graph topology, are simplified.

## Fixing foxtrot merges

It may be simple to fix [foxtrot merges](https://bit-booster.blogspot.com/2016/02/no-foxtrots-allowed.html),
i.e., merges where the mainline branch is not the first parent, by switching the
order of the parents and using `--full-tree`. filter-repo documents earlier
difficulties with foxtrot merges in [a31a381] (filter-repo: delete complex code,
2019-03-14).

[a31a381]: https://github.com/newren/git-filter-repo/commit/a31a381fb81fe3ec7169ee4fcaada8f75505e527

## Modifying files in any commit

filter-repo does not allow filters to modify blobs while processing commits, due
to performance code. (It's discussed in the [`filter-lamely`](https://github.com/newren/git-filter-repo/blob/main/contrib/filter-repo-demos/filter-lamely)
demo.)

### Updating license headers

This would be useful, for example, to automatically update license header years
in any commit that touches a file that had not already been modified that year
and to add license headers to files missing them. With the extended hook
callbacks, the changed blobs could be requested on demand (using `cat-blob` in
the fast-import stream) and modified.

### Splitting files

Since Git has poor support for tracking cross-file textual moves, it may be
desirable to split a file into many files through all of its history.

I've wanted this for when I kept notes in a huge prepend-only Markdown file with
headers by date, to split it into files by date.

### Extracting subsets of files

When maintaining a port of another project, where you want to track upstream
changes, it could be useful to produce a filtered repo with only the definitions
used by the relevant code, so that the diffs are fewer and focused on what you
care about.

For example, filter-repo-rust ports all of the parsing from git fast-import and
it would be useful to filter git.git to just the definitions reachable from
builtin/fast-import.c, such as the oid and date parsing that is reused
elsewhere. This would require language-aware static analysis and code splitting.

## Rewriting quoted commit messages

When rewriting commit messages, commits that quote the header line should be
updated to match.

Git and the Linux kernel use the convention of `%h (%s, %as)`, sometimes with
variation in quotation. The first such commit in git.git is [702088a] (update
'git rebase' documentation, 2008-03-10) and the first in torvalds/linux.git is
[9da1f7e] (powerpc: Do not ignore arch/powerpc/include, 2008-08-07). Both of
these repos could be a good test of this feature.

Surrounding text could be rewrapped when a changed message overflows. This would
require inferring the locally used width or just using 80.

Short hash lengths increase over time, since the number of objects in a repo
also increases. The original quoted lengths should be preserved. (However, a
large repo with many conflicts may benefit from a transformation that increases
the quoted hash length for early commits.)

[702088a]: https://git.kernel.org/pub/scm/git/git.git/commit/?id=702088afc680afef231d4a24bb5890f1d96a2cc9
[9da1f7e]: https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/commit/?id=9da1f7e69aa4545d45d3435865c56f1e67c4b26a
