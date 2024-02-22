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

## Modifying files in any commit

filter-repo does not allow filters to modify blobs while processing commits, due
to performance code. (It's discussed in `contrib/filter-repo-demos/filter-lamely`.)

However, this would be useful, for example, to automatically update license
header years in any commit that touches a file that had not already been
modified that year. With the extended hook callbacks, the changed blobs could
be requested on demand and modified.
