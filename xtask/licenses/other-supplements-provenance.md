# Other dependency licence sources

The eight records in `other-supplements.json` preserve the published crate's licence expression, repository metadata (including the absent repository for `rsqlite-vfs`), registry checksum from `Cargo.lock`, and source commit from `.cargo_vcs_info.json`. Text hashes cover the exact downloaded upstream bytes. These records document sources; they do not constitute a legal certification.

- `rstml` and `rsqlite-vfs`: complete upstream MIT text from the exact source commit. The latter's upstream source repository is `Spxg/sqlite-wasm-rs`; the published package itself has no repository field, which remains null in the record.
- `r-efi`: the exact upstream `AUTHORS` file contains the MIT permission text and the copyright notices, alongside the other offered licences. It is retained in full.
- `wasm_split_helpers` and `wasm_split_macros`: the upstream `LICENSE` files contain Apache-2.0, so that explicitly offered alternative is selected. No missing MIT text is reconstructed.
- `convert_case_extras`, `drain_filter_polyfill`, and `erased`: their source trees at the recorded commits contain no separate licence text. Their complete upstream `Cargo.toml` declarations explicitly offer MIT; the exact declaration files are retained alongside the official SPDX MIT text pinned at commit `31ba1a50e5397e00a304dbadc76531740e89ee48`. The canonical text's copyright placeholders remain unchanged. No copyright holder or year has been invented; the preserved manifests include any upstream author metadata actually present.

The SPDX text is shared with the other supplement groups. Its source URL and byte hash are recorded for each use. No dependency version or lockfile entry was changed while collecting these sources.
