# Connector and browser dependency license sources

`connector-supplements.json` records package declarations, registry checksums from `Cargo.lock` and source commits from `.cargo_vcs_info.json`. Each text has an immutable upstream URL and SHA256. Text bytes and copyright notices remain unchanged. These are offline build inputs.

## MIT selections

Gloo, JNI and rustls Android packages offer `MIT OR Apache-2.0`. Their supplements select MIT using `LICENSE-MIT` at each recorded source commit. `jni-sys` includes its own license texts and needs no supplement.

## WebDriver

`webdriver` 0.53.0 declares `MPL-2.0`. Its `src/lib.rs` includes Mozilla's version 2.0 notice and links to `http://mozilla.org/MPL/2.0/`. Its recorded source commit is `2c83250e67d85c2a172670b08cd88753d1552d91`.

The license text source is Mozilla's official `mozilla/bedrock` repository at commit `a15178c3c7c976c67b3641af77cae0b66093a175`, path `media/MPL/2.0/index.txt`. It is the unmodified MPL 2.0 text. This URL identifies the license text independently from the package source revision. The unavailable package revision is not represented as the origin of this text. No attribution is substituted.
