# WIP Git contributions

## fast-import parsing hardening

Every single usage of `strto*` in fast-import misses some error handling. Here
are the cases I've identified:

1. Unchecked `errno` after call
2. Unchecked end bound, allowing junk after number
3. Allowing an unintended extra sign (i.e., `+` for `strtou*` and `+`/`-` for
   `strto*`)

Many places do not fully consider NUL and assume they are working with regular,
NUL-terminated strings. `read_next_command` (via `strbuf_getline_lf`) reads an
LF-terminated string, and stores it in a buffer with a length.

4. Overrunning NUL

## Docs

The `commit` command has an `encoding` sub-command, yet states this in the
fast-import docs:

> Currently they must be encoded in UTF-8, as fast-import does not permit other
> encodings to be specified.
