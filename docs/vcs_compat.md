# fast-import/fast-export compatibility by VCS

## BitKeeper

[bk fast-import](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c)
([manual](https://www.bitkeeper.org/man/fast-import.html)) supports only `blob`,
`commit`, `reset`, and `progress` commands. Since it is unlikely to change and
bkbits.net is offline, I have linked directly to the relevant lines on GitHub.

- `blob` command is supported, but:
  - `original-oid` directive is not supported.
  - Delimited `data` is [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L1201).
- `commit` command is supported, but:
  - `<ref>` field is [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L550).
  - `original-oid` directive is not supported.
  - `encoding` directive is not supported.
  - Non-standard: `from` and `merge` are [interchangeable](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L592-L597)
    and may appear any number of times interleaved with each other.
  - `M` (modify) changes are supported, but:
    - `160000` gitlinks (and thus submodules) are [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L699-L702).
    - `040000` directories are [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L704-L705).
  - `R` (rename) changes are parsed, but [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L40-L41).
    Since BitKeeper records renames, but Git computes renames with heuristics,
    there are [difficulties](https://users.bitkeeper.org/t/using-fast-import-from-git-into-bk/907)
    in making incremental imports from Git to BitKeeper be inconsistent.
  - `C` (copy) and `deleteall` changes are parsed, but [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L628-L634).
  - `N` (note) changes are [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L715-L721).
- `reset` command is supported, but:
  - `<ref>` field is [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L1043).
- `progress` command is supported.
- In general:
  - Non-standard: Any number of optional LFs are [allowed](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L351-L359)
    between commands.
  - Incremental imports are [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L34).
  - Octopus merges are [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L1731-L1735).
  - Other correctness problems, performance improvements, and features are
    listed in the [TODO](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L17-L42)
    and [a discussion](https://users.bitkeeper.org/t/bk-fast-import-alpha-release/141)
    on its first release.

## Reposurgeon

Reposurgeon parses fast-export streams (with its own extensions) and Subversion
dumps with [`StreamParser`](https://gitlab.com/esr/reposurgeon/-/blob/b1739ef8b9ee6b38230d9d2fede343352dca2d6e/surgeon/inner.go#L4363).
Its fast-export parsing is in [`(*StreamParser).parseFastImport`](https://gitlab.com/esr/reposurgeon/-/blob/b1739ef8b9ee6b38230d9d2fede343352dca2d6e/surgeon/inner.go#L4577)
and is detailed here:

- `blob` command is supported.
- `commit` command:
  - Extension: `'#legacy-id' SP <legacy-id> LF` directive.
  - Non-standard: Multiple `author` directives are supported.
  - Extension: `property` directive, where:
    - `'property' SP <name> SP <value> LF` sets property `<name>` on the commit
      to `"true"`, ignoring `<value>`.
    - `'property' SP <name> SP <count> SP .{count} LF` sets property `<name>` on
      the commit to a value.
  - Non-standard: `from` and `merge` directives are interchangeable and may
    appear any number of times.
  - Non-standard: Directives and changes are parsed in any order. The last
    appearance of a directive is used (including for `property`), except for
    `author`, `from`, and `merge`, which allow multiple. It stops consuming
    lines when an unrecognized directive is encountered or an optional LF. A
    line of only Unicode spaces is skipped.
  - `ls` and `cat-blob` are not supported in the changes list.
  - Bug: All fields in changes can be quoted.
- `tag` command:
  - Extension: `#legacy-id' SP <legacy-id> LF` directive. Appears first.
  - `mark` directive is not supported.
  - `tagger` is required.
- `reset` command is supported.
- `progress` command is skipped.
- `done` command is supported.
- `feature` command is passed through by `StreamParser` and
  `(*Repository).fastExport` suppresses dumping any features that a VCS is known
  to not support, when they are not used.
- `ls`, `cat-blob`, `get-mark`, `checkpoint`, `alias`, `feature`, and `option`
  commands are not supported, but are passed through.
- `data` directive:
  - Extension: A counted `property` directive, i.e.,
    `'property' SP <name> SP <count> SP .{count} LF LF?`, may be used in place
    of a `data` directive. Its `<name>` is ignored in this usage. Note, this
    does not apply for commit messages, because it handles `property`
    separately.
- In general:
  - Non-standard: Any number of blank lines (whitespace-only) are allowed
    between commands.
  - Extension: `'#reposurgeon' SP 'sourcetype' SP <vcs>` directive hints at the
    source VCS and may appear anywhere.
  - `original-oid` directive parses as a Git hash, even though many VCSes are
    supported.
  - Any line that is not supported is passed through.
  - Bug: It is over-permissive with whitespace, allowing Unicode whitespace
    instead of just ASCII SP and often doesn't check for space after a directive
    name.
  - The entire stream, including all blobs, is parsed into memory.
  - Tracks line numbers for error reporting.

Reposurgeon calls commands “events” and (person) identifiers “attributions”.
