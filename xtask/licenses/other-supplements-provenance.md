# Other dependency license sources

The eight records in `other-supplements.json` preserve each package's license expression, repository metadata, locked registry checksum and `.cargo_vcs_info.json` source commit. Text hashes cover exact upstream bytes.

## Package declarations

* `rstml` and `rsqlite-vfs` use complete MIT texts from their exact source commits. The source repository for `rsqlite-vfs` is `Spxg/sqlite-wasm-rs`. Its published package has no repository field, which remains null in the record.
* `r-efi` uses the complete upstream `AUTHORS` file containing MIT permission text, copyright notices and the other offered licenses.
* `wasm_split_helpers` and `wasm_split_macros` select the offered Apache 2.0 license from their upstream `LICENSE` files.
* `convert_case_extras`, `drain_filter_polyfill` and `erased` declare MIT in their complete upstream `Cargo.toml` files and have no separate license text at the recorded revisions. The supplements retain those declarations alongside the official SPDX MIT text at commit `31ba1a50e5397e00a304dbadc76531740e89ee48`.

The SPDX text retains its copyright placeholders. No copyright holder or year is invented. Preserved manifests retain available author metadata. Each use records the canonical text's source URL and byte hash.
