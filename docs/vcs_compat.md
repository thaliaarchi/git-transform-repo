# fast-import/fast-export compatibility by VCS

## BitKeeper

[bk fast-import](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c)
([manual](https://www.bitkeeper.org/man/fast-import.html)) supports only `blob`,
`commit`, `reset`, and `progress` commands.

- `blob` commands are supported, but:
  - `original-oid` directive is not supported.
  - Delimited `data` is [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L1201).
- `commit` commands are supported, but:
  - `<ref>` field is [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L550).
  - `original-oid` directive is not supported.
  - `encoding` directive is not supported.
  - Unlike Git, there may be any number of `from` directives, and `from` and
    `merge` directives are parsed interchangeably in any order [in a loop](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L592-L597).
  - `M` (modify) changes are supported, but:
    - `160000` gitlinks (and thus submodules) are [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L699-L702).
    - `040000` directories are [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L704-L705).
  - `R` (rename) changes are parsed, but [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L40-L41).
    Since BitKeeper records renames, but Git computes renames with heuristics,
    there are [difficulties](https://users.bitkeeper.org/t/using-fast-import-from-git-into-bk/907)
    in making incremental imports from Git to BitKeeper be inconsistent.
  - `C` (copy) and `deleteall` changes are parsed, but [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L628-L634).
  - `N` (note) changes are [not supported](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L715-L721).
- `reset` commands are supported, but:
  - `<ref>` field is [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L1043).
- `progress` commands are fully supported.
- In general:
  - Unlike Git, any number of optional LFs are [allowed](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L351-L359)
    between any commands. Git does not allow optional LF after `blob` or
    repeated LF otherwise.
  - Incremental imports are [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L34).
  - Octopus merges are [not implemented](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L1731-L1735).
  - Other correctness problems, performance improvements, and features are
    listed in the [TODO](https://github.com/bitkeeper-scm/bitkeeper/blob/0524ffb3f6f15ae8d3922b28da581f334475fe61/src/fast-import.c#L17-L42)
    and [a discussion](https://users.bitkeeper.org/t/bk-fast-import-alpha-release/141)
    on its first release.
