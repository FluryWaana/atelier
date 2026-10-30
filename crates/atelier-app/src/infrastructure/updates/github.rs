use super::super::{installed, package, package_archive, update_package};
use crate::Result;
use atelier_core::application::updates::{Download, Operation, Release};
use serde::Deserialize;
use std::{path::Path, time::Duration};

const REPOSITORY: &str = "https://github.com/FluryWaana/atelier/releases/download/";
static DOWNLOAD: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
static PROGRESS: std::sync::Mutex<Option<Download>> = std::sync::Mutex::new(None);

pub(super) fn progress() -> Option<Download> {
    PROGRESS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn report(value: Option<Download>) {
    *PROGRESS.lock().unwrap_or_else(|error| error.into_inner()) = value;
}

struct DownloadGuard;
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        report(None);
    }
}

fn trusted_download(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}
#[derive(Deserialize)]
struct Published {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

fn client(download: bool) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("atelier-kanban-updater")
        .https_only(true)
        .redirect(if download {
            reqwest::redirect::Policy::custom(|attempt| {
                let url = attempt.url();
                if attempt.previous().len() < 5 && trusted_download(url) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            })
        } else {
            reqwest::redirect::Policy::none()
        })
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(if download { 300 } else { 20 }))
        .build()?)
}

async fn bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    receive(&mut response, limit, None).await
}

async fn receive(
    response: &mut reqwest::Response,
    limit: usize,
    total: Option<u64>,
) -> Result<Vec<u8>> {
    if response.status() != reqwest::StatusCode::OK {
        return Err("message.github_update_unavailable".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "message.github_update_unavailable")?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("message.github_update_invalid".into());
        }
        bytes.extend_from_slice(&chunk);
        if let Some(total) = total {
            report(Some(Download {
                received: bytes.len() as u64,
                total,
            }));
        }
    }
    Ok(bytes)
}

async fn latest() -> Result<Option<Published>> {
    fetch_release(
        &client(false)?,
        "https://api.github.com/repos/FluryWaana/atelier/releases/latest",
    )
    .await
}

async fn fetch_release(client: &reqwest::Client, url: &str) -> Result<Option<Published>> {
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| "message.github_update_unavailable")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let release: Published = serde_json::from_slice(&bounded(response, 1024 * 1024).await?)
        .map_err(|_| "message.github_update_invalid")?;
    if release.draft || release.prerelease {
        return Err("message.github_update_invalid".into());
    }
    package::version_parts(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )?;
    Ok(Some(release))
}

fn assets<'a>(release: &'a Published, target: &str) -> Result<(&'a Asset, &'a Asset)> {
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let archive = format!("atelier-{version}-{target}.zip");
    let signature = format!("{archive}.signed.json");
    let find = |name: &str, limit: usize| -> Result<&Asset> {
        let mut found = release.assets.iter().filter(|asset| asset.name == name);
        let asset = found.next().ok_or("message.github_update_incompatible")?;
        if found.next().is_some() || asset.size == 0 || asset.size > limit as u64 {
            return Err("message.github_update_invalid".into());
        }
        let url = reqwest::Url::parse(&asset.browser_download_url)?;
        let expected = format!("{REPOSITORY}{}/{name}", release.tag_name);
        if url.as_str() != expected {
            return Err("message.github_update_invalid".into());
        }
        Ok(asset)
    };
    Ok((
        find(&archive, package_archive::ARCHIVE_LIMIT)?,
        find(&signature, 16 * 1024)?,
    ))
}

pub async fn check_release(current: &str, target: &str) -> Result<Option<Release>> {
    select_release(latest().await?, current, target)
}

fn select_release(
    release: Option<Published>,
    current: &str,
    target: &str,
) -> Result<Option<Release>> {
    let Some(release) = release else {
        return Ok(None);
    };
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    if package::version_parts(version)? <= package::version_parts(current)? {
        return Ok(None);
    }
    Ok(Some(Release {
        version: version.into(),
        compatible: assets(&release, target).is_ok(),
    }))
}

pub async fn install_release(
    root: &Path,
    version: &str,
    data: &Path,
    port: u16,
) -> Result<Operation> {
    let _permit = DOWNLOAD
        .try_acquire()
        .map_err(|_| "message.github_update_busy")?;
    let _progress = DownloadGuard;
    super::require_user_update(root)?;
    if super::status(Some(root))?
        .operation
        .is_some_and(|operation| operation.state.pending())
    {
        return Err(
            "message.a_preparation_or_update_already_exists_review_or_cancel_it_first".into(),
        );
    }
    let target = package::native_target()?;
    let release = latest().await?.ok_or("message.github_update_unavailable")?;
    if release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        != version
    {
        return Err("message.github_update_changed".into());
    }
    let (archive, signature) = assets(&release, target)?;
    let installation = installed::open(root, target)?
        .ok_or("message.this_command_requires_a_versioned_rust_installation")?;
    if package::version_parts(version)?
        <= package::version_parts(&installation.current().active.version)?
    {
        return Err("message.this_update_is_not_newer_than_the_active_version".into());
    }
    let client = client(true)?;
    let metadata = bounded(
        client
            .get(&signature.browser_download_url)
            .send()
            .await
            .map_err(|_| "message.github_update_unavailable")?,
        16 * 1024,
    )
    .await?;
    let payload = update_package::authenticated_payload(&metadata, installation.trust(), target)?;
    if payload.version != version || payload.archive_size != archive.size {
        return Err("message.github_update_invalid".into());
    }
    drop(installation);
    report(Some(Download {
        received: 0,
        total: archive.size,
    }));
    let bytes = receive(
        &mut client
            .get(&archive.browser_download_url)
            .send()
            .await
            .map_err(|_| "message.github_update_unavailable")?,
        archive.size as usize,
        Some(archive.size),
    )
    .await?;
    if bytes.len() as u64 != archive.size {
        return Err("message.github_update_invalid".into());
    }
    let temporary = tempfile::tempdir()?;
    let archive_path = temporary.path().join("update.zip");
    let signature_path = temporary.path().join("update.signed.json");
    std::fs::write(&archive_path, bytes)?;
    std::fs::write(&signature_path, metadata)?;
    let operation = super::preview(root, &archive_path, &signature_path, data, port).await?;
    super::apply(root, &operation.id, true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn published(version: &str) -> Published {
        Published {
            tag_name: version.into(),
            draft: false,
            prerelease: false,
            assets: Vec::new(),
        }
    }

    async fn server(
        status: &str,
        body: &[u8],
        declared: usize,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/release", listener.local_addr().unwrap());
        let reply = [
            format!("HTTP/1.1 {status}\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n")
                .into_bytes(),
            body.to_vec(),
        ]
        .concat();
        let task = tokio::spawn(async move {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).await.unwrap();
            let _ = stream.write_all(&reply).await;
        });
        (url, task)
    }

    #[test]
    fn version_comparison_is_numeric_and_never_offers_a_downgrade() {
        let target = package::TARGETS[0];
        for (current, latest, newer) in [
            ("0.1.9", "v0.1.10", true),
            ("0.1.10", "0.1.9", false),
            ("1.2.3", "v1.2.3", false),
            ("1.9.9", "v2.0.0", true),
        ] {
            let selected = select_release(Some(published(latest)), current, target).unwrap();
            assert_eq!(selected.is_some(), newer);
            if let Some(selected) = selected {
                assert!(!selected.compatible);
            }
        }
        assert!(select_release(None, "1.2.3", target).unwrap().is_none());
        for invalid in [
            "v1.2.3-rc.1",
            "1.2",
            "01.2.3",
            "../../1.2.3",
            "1.2.3?secret=x",
        ] {
            assert!(select_release(Some(published(invalid)), "1.0.0", target).is_err());
        }
    }

    #[test]
    fn redirects_only_accept_https_github_asset_hosts_without_credentials() {
        for url in [
            "https://github.com/a",
            "https://release-assets.githubusercontent.com/a?signature=synthetic",
            "https://objects.githubusercontent.com/a",
        ] {
            assert!(trusted_download(&reqwest::Url::parse(url).unwrap()));
        }
        for url in [
            "http://github.com/a",
            "https://github.com.evil.test/a",
            "https://127.0.0.1/a",
            "https://github.com:444/a",
            "https://user:secret@github.com/a",
            "https://evil.test/github.com/a",
        ] {
            assert!(!trusted_download(&reqwest::Url::parse(url).unwrap()));
        }
    }

    #[tokio::test]
    async fn github_http_errors_invalid_metadata_and_missing_releases_are_distinct() {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        for status in [
            "403 Forbidden",
            "429 Too Many Requests",
            "500 Internal Server Error",
            "302 Found",
        ] {
            let (url, task) = server(status, b"secret must not appear", 22).await;
            let error = fetch_release(&client, &url)
                .await
                .err()
                .unwrap()
                .to_string();
            assert_eq!(error, "message.github_update_unavailable");
            task.await.unwrap();
        }
        let (url, task) = server("404 Not Found", b"", 0).await;
        assert!(fetch_release(&client, &url).await.unwrap().is_none());
        task.await.unwrap();
        for body in [
            br#"{"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[]}"#.as_slice(),
            br#"{"tag_name":"v1.2.3","draft":true,"prerelease":false,"assets":[]}"#,
            br#"{"tag_name":"v1.2.3","draft":false,"prerelease":true,"assets":[]}"#,
            b"not json",
            b"{}",
        ] {
            let (url, task) = server("200 OK", body, body.len()).await;
            let result = fetch_release(&client, &url).await;
            assert_eq!(
                result.is_ok(),
                body == br#"{"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[]}"#
            );
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn downloaded_bodies_are_bounded_and_incomplete_transfers_fail_without_exposing_urls() {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        for (body, declared, limit, accepted) in [
            (b"abcd".as_slice(), 4, 4, true),
            (b"abcde", 5, 4, false),
            (b"abc", 20, 100, false),
        ] {
            let (url, task) = server("200 OK", body, declared).await;
            let result = bounded(client.get(&url).send().await.unwrap(), limit).await;
            assert_eq!(result.is_ok(), accepted);
            if let Err(error) = result {
                assert!(!error.to_string().contains(&url));
            }
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn concurrent_install_is_rejected_before_network_or_data_access() {
        let _permit = DOWNLOAD.acquire().await.unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let error = install_release(
            temporary.path(),
            "1.2.3",
            &temporary.path().join("must-not-open"),
            4328,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "message.github_update_busy");
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn progress_reports_received_bytes_and_clears_on_success_or_failure() {
        let _permit = DOWNLOAD.acquire().await.unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        for (body, limit, accepted) in [(b"abcd".as_slice(), 4, true), (b"abcde", 4, false)] {
            {
                let _guard = DownloadGuard;
                report(Some(Download {
                    received: 0,
                    total: 4,
                }));
                assert_eq!(progress().unwrap().received, 0);
                let (url, task) = server("200 OK", body, body.len()).await;
                let result =
                    receive(&mut client.get(&url).send().await.unwrap(), limit, Some(4)).await;
                assert_eq!(result.is_ok(), accepted);
                let observed = progress().unwrap();
                assert_eq!(observed.total, 4);
                assert!(observed.received <= observed.total);
                if accepted {
                    assert_eq!(observed.received, 4);
                }
                task.await.unwrap();
            }
            assert!(progress().is_none());
        }
    }

    #[test]
    fn release_requires_unique_platform_assets_from_the_official_repository() {
        let target = "x86_64-pc-windows-msvc";
        let name = format!("atelier-1.2.3-{target}.zip");
        let asset = |name: String| Asset {
            browser_download_url: format!("{REPOSITORY}v1.2.3/{name}"),
            name,
            size: 100,
        };
        let mut release = Published {
            tag_name: "v1.2.3".into(),
            draft: false,
            prerelease: false,
            assets: vec![asset(name.clone()), asset(format!("{name}.signed.json"))],
        };
        assert!(assets(&release, target).is_ok());
        assert!(assets(&release, "aarch64-apple-darwin").is_err());
        release.assets[0].browser_download_url = "https://example.com/update.zip".into();
        assert!(assets(&release, target).is_err());
        release.assets[0] = asset(name.clone());
        release.assets.push(asset(name));
        assert!(assets(&release, target).is_err());
    }

    #[tokio::test]
    async fn downloaded_signed_package_is_prepared_only_when_all_bytes_match() {
        let (root, trust) = update_package::tests::fixture();
        let metadata = std::fs::read(root.path().join("update.json")).unwrap();
        let archive = std::fs::read(root.path().join("update.zip")).unwrap();
        let target = package::TARGETS[1];
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let (url, task) = server("200 OK", &metadata, metadata.len()).await;
        let downloaded_metadata = bounded(client.get(url).send().await.unwrap(), 16 * 1024)
            .await
            .unwrap();
        task.await.unwrap();
        let payload =
            update_package::authenticated_payload(&downloaded_metadata, &trust, target).unwrap();
        assert_eq!(payload.archive_size, archive.len() as u64);
        let metadata_path = root.path().join("downloaded.json");
        std::fs::write(&metadata_path, downloaded_metadata).unwrap();
        let download = root.path().join("downloaded.zip");
        let staging = root.path().join("staging");
        for corrupt in [false, true] {
            let mut body = archive.clone();
            if corrupt {
                body[20] ^= 1;
            }
            let (url, task) = server("200 OK", &body, body.len()).await;
            let bytes = bounded(
                client.get(url).send().await.unwrap(),
                payload.archive_size as usize,
            )
            .await
            .unwrap();
            task.await.unwrap();
            std::fs::write(&download, bytes).unwrap();
            let prepared = update_package::prepare(
                &download,
                &metadata_path,
                &trust,
                target,
                "0.1.9",
                &staging,
            );
            assert_eq!(prepared.is_ok(), !corrupt);
            drop(prepared);
            assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0);
        }
        assert!(
            update_package::authenticated_payload(&metadata, &trust, package::TARGETS[0]).is_err()
        );
        let foreign = root.path().join("foreign-keys");
        update_package::generate_synthetic(&foreign).unwrap();
        let foreign =
            update_package::Trust::load(&foreign.join(update_package::PUBLIC_KEY)).unwrap();
        assert!(update_package::authenticated_payload(&metadata, &foreign, target).is_err());
    }
}
