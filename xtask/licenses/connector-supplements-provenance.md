# Connector and browser dependency license sources

`connector-supplements.json` records the exact package declarations, registry
checksums from `Cargo.lock`, and commits from the downloaded packages'
`.cargo_vcs_info.json`. The upstream files were copied as bytes, without changing
copyright notices or line endings. Each file has its SHA-256 and immutable source
URL in the manifest. These are build-time inputs; packaging needs no runtime
network request.

The Gloo, JNI and rustls Android packages offer `MIT OR Apache-2.0`; these
supplements explicitly select MIT and contain their upstream `LICENSE-MIT` at
each package's recorded commit. `jni-sys` itself already includes its license
texts in its registry archive and needs no supplement.

`webdriver` 0.53.0 declares `MPL-2.0`. Its registry source `src/lib.rs` starts with
Mozilla's MPL version 2.0 notice and links to `http://mozilla.org/MPL/2.0/`.
Its recorded source commit is `2c83250e67d85c2a172670b08cd88753d1552d91`.
The official Firefox Git mirrors did not resolve that commit, and the historical
Mercurial raw-file endpoint returned revision-not-found. We therefore do not
claim to have recovered a license file from that unavailable source revision.

Instead, the manifest includes Mozilla's own unmodified MPL version 2.0 text
from its official website source repository, `mozilla/bedrock`, at commit
`a15178c3c7c976c67b3641af77cae0b66093a175`, path `media/MPL/2.0/index.txt`.
The source package checksum, its original commit and repository remain recorded
separately from that license-text URL. No missing copyright attribution was
invented or substituted.
