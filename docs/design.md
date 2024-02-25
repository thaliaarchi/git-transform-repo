# git-transform-repo design ideas

Ideas for a git-filter-repo successor for advanced repo transformations. It aims
to make iterative refinement of repo transformations easier.

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

## Front-ends

### Piped fast-export streams

filter-repo exposes no way to ingest a fast-export stream. This would be useful
as a part of a pipeline, such as directly processing a repo converted with
hg-fast-export.

### Libraries

fast-export-rust should make it easy to release your tool as both a binary that
emits fast-export streams and as a Rust library.

Push or pull? Both? With push, the producer can write stateful code easily and
the consumer (e.g., git-transform-repo) needs to use a state machine setup, if
state is needed. The roles of state are flipped for pull. I usually prefer to
write pull parsers, as they give greater flexibility to the consumer. Since pull
parsers can be wrapped to become push parsers, fast-export-rust could provide
traits for both, an adaptor for converting pull to push, and a way to easily
dump the stream in `main` given either.

### Releases from archives

I've needed to construct repos from tar and zip releases of projects many times.
When trying to make a useful history more granular than just using the dates of
the releases, file metadata inside the archives can be used. Files can be
grouped into commits by date modified, such as by hours.

It gets complicated when the provenance of changes is messy, such as with the
Inferno OS 1E–3E releases. There are two versions available for 1E: a version of
a beta build which was modified later ([“1e0”](https://web.archive.org/web/20110807071440/http://www.vitanuova.com/dist/old/1e/1e0.tgz))
and the 1.0 release ([“1e1”](https://web.archive.org/web/20110807071535/http://www.vitanuova.com/dist/old/1e/1e1src.tgz)).
1e0 has earlier timestamps for many files, and those could be fashioned into
commits on the main branch. Anything later than 1e1 would be in a separate
branch. Then, 1e1 would continue off of the first part of 1e0. Tracking all of
this by hand is messy, and I want to automate it.

A front-end could export the contents of a tar as a fast-export stream. As it
reads files, it would immediately stream them as blobs, marked so they can be
referenced later. The pertinent metadata would be collected, then after all
blobs have been dumped, it would analyze and generate commits. With a zip, it
could dump blobs on demand, if that is useful, since it has an index.

This wouldn't need to be folded into git-transform-repo, since it would emit a
fast-export stream. However, if fast-export-rust makes it easy to publish a tool
as as a binary and library, it could be plugged in directly.

### Other VCSes

It may be worth porting hg-fast-export for stability and control, as I ran into
[a change](https://github.com/thaliaarchi/repo-archival/commit/890abd5d36c1f5bfce81f5f884d42835f6e57e0e)
which now produces different output from before. Although that change may have
improved correctness for future conversions, it is now different from existing
conversions that seem to have used the tool (or some other tool that happens to
produce the same output). I usually work with old repos and need
reproducibility.

### Remote machines

It could be useful to export a repo on one machine, wrap it in some transport
like TLS, and import it on another. I suspect centralized VCSes like SVN might
benefit from this, where traditional approaches to exporting have checked out
each revision in a very slow process.

The stream would need compression. The fast-export format is designed for
simplicity of implementation, so blobs are not compressed like in packfiles and
would be very large. The stream could be sent in chunks and compressed as a
whole, or each blob could be compressed. Then on the receiving side, a decoder
would decompress and concatenate the chunks, piping it to a local consumer as a
regular fast-export stream. This bridge tool would enable extensions to the
format for transport purposes, because that would be a private implementation
detail (well, until someone else writes their own tool using this transport
protocol).

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

Diffs of histories, instead of trees would also be invaluable for rebase
workflows, to understand the changes across the commit graph between before and
after a rebase.

A text format like git log would be easy to throw together, once the diff
mechanics are worked out, but a GUI with editor integration would be very useful
for viewing the changes hierarchically.

## Transformation DSL

My repo transformations are maintained over longer periods of time than the
filter-repo was designed for, since it is intended for one-time scripts. I'd
love to have an expressive DSL that can declare the steps in a repo
transformation, similarly to reposurgeon.

### Rebase-like actions

The interactive rebase todo list works well for writing steps to modify commits
and the DSL could take inspiration from it and replace some of its use cases.
For example, the command `reword 0123abc "New commit message"` could replace the
message of the identified commit, `reword 0123abc` could open the message in
your `core.editor` for editing, and `reword 0123abc <<EOF` could start a
heredoc-style message. Replace patterns could be supported too, like
`reword 0123abc /old ([^ ]+)/ "new \1"`.

## Callback hooks

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

If this is generalized, getters and setters could replace the properties used by
filter-repo. Then there would be no distinction between data coming from the
current object in the fast-export stream and data that needs to be retrieved
from elsewhere. This also makes it more efficient from the Rust side, because
then values can be copied to Python on demand.

Rather than exposing these hooks as methods on a parameter to the callback, they
can be functions in scope within the callback's globals.

Getters and setters could share the same function name. The argument for a
setter would be a keyword argument, so it can be omitted in the getter.

To maintain backwards compatibility with filter-repo, the old callbacks would
keep their same flags like `--commit-callback` and the new style would be named
differently. Perhaps `--process-commit`, `--commit-hook`, `--handle-commit`, or
`--on-commit`?

## Scripting

Scripting is essential for easy extensibility.

### Using a scripting language

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

jq is great for declaratively transforming streams in pipelines. That would make
it a poor fit for the callbacks, though, which process a single piece of data at
a time. If the jq filter operated on just a single value, most of its benefits
disappear, and if the values were streamed, jq would need to emit the same
number of outputs, making it fragile. It would be better as an external
processor of JSON exported from git-transform-repo.

If it's useful to support multiple scripting languages, callbacks could be
tagged with the language, such as `--commit-callback:py '…'`.

### Using Rust

Since it's written in Rust, library users could supply Rust callbacks.

To support both Rust and Python callbacks, callbacks would be stored as a
three-variant enum (`Rust`, `Python`, and `None`). When it's Rust, a cheaper
representation without copying would be passed to the callback. If the two APIs
are similar enough, perhaps the Rust representation could be automatically
wrapped for Python with codegen similarly to bindgen.
