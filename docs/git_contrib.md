# WIP Git contributions

## fast-import parsing hardening

### Integer parsing

Every single usage of `strto*` in fast-import misses some error handling. Here
are the cases I've identified:

1. Unchecked `errno` after call
2. Unchecked end bound, allowing junk after number
3. Allowing an unintended extra sign (i.e., `+` for `strtou*` and `+`/`-` for
   `strto*`)
4. Overrunning NUL

### Path parsing

Paths are parsed in four ways by fast-import and do not handle all parse errors.
See [parsing discrepancies](./parsing_discrepancies.md).

### Truncating strings at NUL

Many places do not fully consider NUL and assume they are working with regular,
NUL-terminated strings. `read_next_command` (via `strbuf_getline_lf`) reads an
LF-terminated string, and stores it in a buffer with a length.

### `strbuf` memory leaks

`strbuf`s, such as those created for unquoting strings with `unquote_c_style`,
do not seem to free their buffers.

## fsck

`tree-walk.c:get_mode` does not check for integer overflow when shifting the
mode `unsigned int`. `badDateOverflow` checks for integer overflow for dates, so
this seems to be an oversight.

fsck `badTimezone` could be loosened to only check that the timezone is digits
and a separate new `badTimezoneLength` warning could detect those that do not
have exactly 4 digits.

It has been [suggested](https://lore.kernel.org/git/xmqq7dnpc610.fsf@gitster.c.googlers.com/)
to refactor some of the fast-import parsing to reuse fsck machinery. Perhaps
this would be useful for idents.

For tags, `fsck_ident` should report “invalid tagger line” instead of “invalid
author/committer line”.

## Docs

The `commit` command has an `encoding` sub-command, yet states this in the
fast-import docs:

> Currently they must be encoded in UTF-8, as fast-import does not permit other
> encodings to be specified.

`feature` is missing `alias`, `rewrite-submodules-to`, and
`rewrite-submodules-from`.

Upstream consistent command/directive/line terminology.

`fullPathname` for `git fsck` is [described](https://git-scm.com/docs/git-fsck#_fsck_messages)
as “A path contains the full path starting with "/".”, but it checks for this
with `has_full_path |= !!strchr(name, '/');` on the path of a tree entry.
