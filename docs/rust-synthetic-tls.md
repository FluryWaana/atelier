# Synthetic TLS tests

TLS fixtures exercise real adapters against temporary HTTPS servers without a real account. They use synthetic databases, repositories, tokens and compiled agents.

## Integration transport

```text
cargo test -p atelier-app --test synthetic-tls --locked -- --ignored
```

The ignored tests require `CI=true` and `ATELIER_RUST_FINAL_CAMPAIGN=1` inside the intended disposable environment. They cover Jira reads and import, preservation of human card content, human time entries, Tempo preview and confirmation, interrupted responses and reconciliation without another POST. Forge reads verify exact site, repository and request identity.

These are native adapter and use case tests. They do not establish configuration screen behavior or browser TLS coverage.

## Certificates and isolation

OpenSSL generates a temporary CA and short lived certificates in a private OS temporary directory. The executable uses an absolute platform path, explicit configuration, cleared environment and temporary HOME. Missing OpenSSL fails the test. No trust store, DNS configuration, hosts file or personal keychain is modified.

Servers bind to `127.0.0.1` on ephemeral ports. Synthetic clients require HTTPS, explicit loopback address and port, a `synthetic-` token and the supplied CA. They disable proxies, redirects and implicit retry while keeping TLS verification enabled. Foreign CAs, incorrect certificate addresses and nonlocal origins are rejection cases.

These constructors are test fixtures and are not exposed through application routes or runtime settings.

## Publication and review

```text
cargo test -p xtask --test forge-review-tls --locked -- --ignored
```

This scenario uses a managed worktree, real synthetic commits, a TLS forge API and `git http-backend`. Compiled fake agents exercise development, verification, confirmed publication, an interrupted creation response, reconciliation, review and source revalidation. Human criteria remain unchecked.

Assertions verify HEAD, index and FETCH_HEAD preservation and reject wrong TLS identity, another repository and changed source content. The fixture observes actual processes rather than injecting a completed agent report.

Git uses an explicit CA bundle in its temporary HOME, exact origin credential scope and `sslVerify=true`. Linux retains its provisioned TLS backend. Windows and macOS require an effective OpenSSL capable Git/curl setup. The fixture must verify the actual backend rather than assume a requested setting was honored.

Certificates, fixture servers and private keys remain outside product packages. See [verification](rust-final-qualification.md) for platform and disposable environment requirements.
