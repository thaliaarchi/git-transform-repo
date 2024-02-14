# Notes on git-filter-repo

## Interleaving repos

- [a594ea5] 2009-02-17 filter-repo: ensure new files from spliced-in commits aren't dropped at merges
- [dd5665b] 2009-02-21 filter-repo: handle adding interleaving commits from separate repositories
- [7437d62] 2009-02-23 filter-repo: fix id renaming
- [e7311b6] 2009-04-04 filter-repo: reinstate the id_offset
- [8101682] 2019-01-07 filter-repo: allow chaining of RepoFilter instances
- [72b69b3] 2019-01-08 filter-repo: support --source and --target options
- [a5d4d70] 2018-11-05 filter-repo: add some testcases making use of filter-repo as a library
- [a31a381] 2019-03-14 filter-repo: delete complex code
- [7d42c20] 2019-05-31 filter-repo: limit splicing repos warning to test that splices repos

t/t9391/splice_repos.py demonstrates a limited case of splicing histories.

[a594ea5]: https://github.com/newren/git-filter-repo/commit/a594ea530abe42e1074eb59935e7599bb896fd4e
[dd5665b]: https://github.com/newren/git-filter-repo/commit/dd5665b7ece15065196e1cc6168699aa75e3c6e3
[7437d62]: https://github.com/newren/git-filter-repo/commit/7437d62329e84472e77f6395aad1c5bb50ff744d
[e7311b6]: https://github.com/newren/git-filter-repo/commit/e7311b6db937c6f6927995c35870e5fd92ce9009
[8101682]: https://github.com/newren/git-filter-repo/commit/81016821a1a5b388f3e9f9bf5c612d207db57ed7
[72b69b3]: https://github.com/newren/git-filter-repo/commit/72b69b3dbe9aacc0138245fc61a0a5db1950ab8d
[a5d4d70]: https://github.com/newren/git-filter-repo/commit/a5d4d70876ad51ad710cc800ff34b5a2c326c8aa
[a31a381]: https://github.com/newren/git-filter-repo/commit/a31a381fb81fe3ec7169ee4fcaada8f75505e527
[7d42c20]: https://github.com/newren/git-filter-repo/commit/7d42c2093cd4e6690dda5e9c9a1139d4be9ab69b

## Text replacement precedence

`FilteringOptions.get_replace_text` partitions text replacements into literals
and regexes (globs are converted to regexes). Literals are then replaced in a
group, followed by regexes in a group, in the callbacks `_tweak_blob`,
`_tweak_commit`, and `_tweak_tag`.

Is this sound precedence?

Yeah, I think this is probably how it should be, because an overly general
pattern before a literal would have precedence, which may not be intuitive.
Literals match only one thing, and if a regex that also matches it is evaluated
before it, the literal will never match. The general solution would be a pattern
complexity scheme (like used in Logos) to order evaluation, but that is
unnecessary here.

## Callback second parameter

Callbacks have an undocumented second parameter, named [`_do_not_use_this_var`](https://github.com/newren/git-filter-repo/blob/4bc9022afce5e2e138596bbecf4df310212ae164/git-filter-repo#L2840),
which is named `aux_info` at the call sites or `metadata` in the `contrib/`
library usage.

## Modifying files in any commit

git filter-repo (rightfully) assumes that it is not useful to run filters on all
blobs on all commits, due to the high performance cost. (It's discussed in
`contrib/filter-repo-demos/filter-lamely`.)

However, this would be useful, for example, to automatically update license
header years in any commit that touches a file that had not already been
modified that year.
