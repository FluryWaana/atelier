# Dependency license supplements

The notice generator inventories registry packages in `Cargo.lock`, including build tools and all target platforms. Explicit supplements provide upstream license text for packages that omit it and for identified embedded components. This is an inventory of upstream declarations rather than legal certification.

## Identity and integrity

Each JSON record binds package name, version, declared license, repository metadata, registry checksum and `.cargo_vcs_info.json` revision. Text records include an official source URL and SHA256. Generation is offline and refuses unknown missing texts, changed identities, revisions or bytes.

Texts bundled in crate archives remain included even when a component has an additional supplement. Every generated notice links the exact source archive. Upstream legal texts remain verbatim, including copyright notices, line endings and placeholders. No author or copyright year is invented.

## Sources

* [Connector and browser dependencies](connector-supplements-provenance.md)
* [Leptos dependencies](leptos-supplements-provenance.md)
* [Other dependencies](other-supplements-provenance.md)

The objc2 family retains the complete upstream `LICENSE.md` at each crate revision, including its explanation of Apple SDK derived bindings and its MIT link. `SPDX-MIT.txt` contains the exact standard text at commit `31ba1a50e5397e00a304dbadc76531740e89ee48`. Selecting an offered license does not change an upstream declaration.

## Native components

`libsqlite3-sys` 0.38.2 embeds SQLite 3.53.2, Fossil check in `d6e03d8c777cfa2d35e3b60d8ec3e0187f3e`. The wrapper uses MIT. The separate copyright disclaimer and blessing in `sqlite3/sqlite3.c` remain a complete supplemental notice, bound to the exact source revision and registry checksum.

Recursive collection includes `aws-lc-sys` texts in `aws-lc/LICENSE` and `third_party/fiat/LICENSE`, plus the `LICENSE-*` files in `ring`. Package verification remains a separate check described in [packaging](../../docs/rust-packages.md).
