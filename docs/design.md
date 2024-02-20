# git-transform-repo

Ideas for a git-filter-repo successor for advanced repo transformations,
tentatively called git-transform-repo. It aims for making iterative refinement
of repo transformations easier.

## Input repo

filter-repo assumes that you start with a freshly cloned repo, and verifies that
with various heuristics. This means, that as you refine the filter script, you
end up deleting and cloning or copying many times over, performing much
redundant work. There are also false positives, such as a repo containing paths
that only differ by case on a case-insensitive filesystem, which require passing
`--force`.

If cloning was incorporated into the process, then it would be safer and could
handle caching. However, in my use cases, I've almost always already had a copy
of the repo. As I understand it, though, filter-repo doesn't even use most of
the source repo! It operates on a fast-export stream, which includes the blob
data, so it has everything it needs to perform the filter from the stream alone.

Instead, I think it should write the results to a separate, fresh repo by
default and wouldn't modify the source repo. Modifying the current repo could be
opt-in by passing `--in-place` and, if it is not a fresh clone, `--force`.

## Other input formats

filter-repo exposes no way to ingest a fast-export stream. This would be useful
as a part of a pipeline, such as processing a repo converted with
hg-fast-export.

## Output formats

When filter-repo is used in a pipeline, it is useful to produce a fast-export
stream. Currently, this is possible through using `--dry-run` or `--debug` to
get `.git/filter-repo/fast-export.original` and `fast-export.filtered`, but
there seems to be no way of streaming it. Furthermore, these do not contain blob
data, so a client would need to separately request it, such as with `git
cat-file --batch`.

Producing a stream in some other format such as JSON would allow other tools to
more easily operate on the data. Using jq and fq, in particular, would enable ad
hoc transformations.

Sometimes, it might even be useful to produce other kinds of effects. It would
need a clear specification of the ordering of the callbacks. Maybe this kind of
usage is better left to it as a library, though.

Printing to the console should be better controlled with verbosity levels and
have more useful status information, redrawn with ANSI escape sequences.

## Repo diffs

I usually use filter-repo in an iterative process, refining the resulting repo,
until I am satisfied. If histories could be compared, disregarding commit hashes
when not relevant, the process would be much easier.

## DSL

My repo transformations are maintained over longer periods of time than the
filter-repo docs imply it is designed for, since it is intended for one-time
scripts. I'd love to have an expressive DSL that can declare the steps in a repo
transformation, similarly to reposurgeon.

## Hooks

Additional information, that is not included in the fast-export stream, could be
requested on-demand in callbacks using methods on the object.

In `--commit-callback`, blob data is not available, but it is useful in some
cases. The [lint-history](https://github.com/newren/git-filter-repo/blob/4bc9022afce5e2e138596bbecf4df310212ae164/contrib/filter-repo-demos/lint-history#L170-L172)
demo requests blob data from a batched git cat-file subprocess within a commit
callback. It calls an external command on each blob in the history, but when
file names are important, it needs to traverse commits for the first occurrence
of each blob to get the file names. This could generalized by adding a
`Blob::data` method, that retrieves it on demand.

For user-defined logic for keeping or discarding commits, `Commit::drop` could
be defined.

## Scripting

Scripting is essential for easy extensibility.

### Using a traditional scripting language

Python is a good choice for filter-repo, because it is platform-independent, so
it doesn't suffer from the issues of using shell scripts as in filter-branch.
Python could be embedded into Rust using PyO3 and the two languages work well
together, so it is likely the best option for transform-repo.

Many of the tasks I have used callbacks for are text processing, so perhaps Perl
would be worth looking into. However, Perl is no longer popular and embedding
would likely be difficult.

JavaScript, although popular, would be slow and is not a good choice for working
with byte strings.

Lua is easily embeddable and LuaJIT is fast. Unless its text processing is
better than Python's, it's likely not worth it.

### Using jq

What if jq was the scripting language? jq is very nice for declarative scripts,
and I find jq most useful for defining pipelines to transform streams. It could
be embedded using jaq.

The callbacks in filter-repo operate on a single piece of data at a time (a
filename, message, name, email, refname, blob, commit, or reset). If the jq
filter transformed a single value, most of the benefits of jq disappear, and if
the values were streamed, jq would need to emit the same number of outputs,
which makes it barely different from operating on one at a time.

Efficiency-wise, mutations of the input (as in the Python style) would likely be
more efficient than pure values.

Is there some way the stream paradigm could be useful for transform-repo?

### Multiple languages

If it's useful to support multiple scripting languages, callbacks could be
tagged with the language, such as `--commit-callback:py '…'`.
