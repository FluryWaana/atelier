use atelier_app::{Result, infrastructure::process_tree};
use axum::Router;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
};

pub fn require_final() {
    assert_eq!(std::env::var("CI").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref(),
        Ok("1")
    );
}

pub struct Certificates {
    pub(crate) _directory: tempfile::TempDir,
    pub authority: Vec<u8>,
    pub foreign: Vec<u8>,
    pub valid: Arc<rustls::ServerConfig>,
    pub wrong_host: Arc<rustls::ServerConfig>,
}
impl Certificates {
    pub async fn new() -> Result<Self> {
        require_final();
        let directory = tempfile::Builder::new()
            .prefix("atelier-synthetic-tls-")
            .tempdir()?;
        let root = directory.path();
        std::fs::write(
            root.join("ca.cnf"),
            "[req]\ndistinguished_name=dn\nprompt=no\nx509_extensions=ca\n[dn]\nCN=Atelier synthetic TLS authority\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,cRLSign\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid:always\n",
        )?;
        for (name, ip) in [("leaf.ext", "127.0.0.1"), ("wrong.ext", "127.0.0.2")] {
            std::fs::write(
                root.join(name),
                format!(
                    "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:{ip}\n"
                ),
            )?;
        }
        for (key, certificate) in [("ca.key", "ca.pem"), ("foreign.key", "foreign.pem")] {
            openssl(
                root,
                &[
                    "req",
                    "-x509",
                    "-newkey",
                    "rsa:2048",
                    "-sha256",
                    "-nodes",
                    "-days",
                    "2",
                    "-config",
                    "ca.cnf",
                    "-keyout",
                    key,
                    "-out",
                    certificate,
                ],
            )
            .await?;
        }
        openssl(
            root,
            &[
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-sha256",
                "-nodes",
                "-config",
                "ca.cnf",
                "-subj",
                "/CN=Atelier synthetic loopback",
                "-keyout",
                "server.key",
                "-out",
                "server.csr",
            ],
        )
        .await?;
        for (extension, certificate) in [("leaf.ext", "server.pem"), ("wrong.ext", "wrong.pem")] {
            openssl(
                root,
                &[
                    "x509",
                    "-req",
                    "-sha256",
                    "-in",
                    "server.csr",
                    "-CA",
                    "ca.pem",
                    "-CAkey",
                    "ca.key",
                    "-CAcreateserial",
                    "-days",
                    "2",
                    "-extfile",
                    extension,
                    "-out",
                    certificate,
                ],
            )
            .await?;
        }
        fn config(root: &Path, certificate: &str) -> Result<Arc<rustls::ServerConfig>> {
            let chain = vec![CertificateDer::from_pem_file(root.join(certificate))?];
            let key = PrivateKeyDer::from_pem_file(root.join("server.key"))?;
            let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(chain, key)?;
            config.alpn_protocols = vec![b"http/1.1".to_vec()];
            Ok(Arc::new(config))
        }
        let valid = config(root, "server.pem")?;
        let wrong_host = config(root, "wrong.pem")?;
        Ok(Self {
            authority: std::fs::read(root.join("ca.pem"))?,
            foreign: std::fs::read(root.join("foreign.pem"))?,
            valid,
            wrong_host,
            _directory: directory,
        })
    }
}

pub fn openssl_executable() -> Result<PathBuf> {
    #[cfg(windows)]
    let executable = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .ok_or("ProgramFiles absent pour le scénario TLS.")?
        .join("Git/usr/bin/openssl.exe");
    #[cfg(target_os = "macos")]
    let executable = PathBuf::from(if cfg!(target_arch = "aarch64") {
        "/opt/homebrew/opt/openssl@3/bin/openssl"
    } else {
        "/usr/local/opt/openssl@3/bin/openssl"
    });
    #[cfg(all(unix, not(target_os = "macos")))]
    let executable = PathBuf::from("/usr/bin/openssl");
    if !executable.is_absolute() || !executable.is_file() {
        return Err("OpenSSL OS absolu requis pour le scénario TLS synthétique.".into());
    }
    Ok(executable)
}

async fn openssl(root: &Path, args: &[&str]) -> Result<()> {
    let executable = openssl_executable()?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("LC_ALL", "C")
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("TEMP", root)
        .env("TMP", root)
        .env("TMPDIR", root)
        .env("RANDFILE", root.join("synthetic.rng"))
        .env("OPENSSL_CONF", root.join("ca.cnf"));
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let output =
        process_tree::capture(&mut command, &[], Duration::from_secs(40), 64 * 1024).await?;
    if !output.status.success() {
        return Err(format!(
            "OpenSSL synthétique a refusé l’étape {} (code {:?}).",
            args[0],
            output.status.code()
        )
        .into());
    }
    Ok(())
}

pub struct Server {
    pub base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    pub async fn start(config: Arc<rustls::ServerConfig>, router: Router) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("https://{}", listener.local_addr()?);
        let acceptor = TlsAcceptor::from(config);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let Ok((stream,peer))=accepted else{return;};assert!(peer.ip().is_loopback());
                        let acceptor=acceptor.clone();let router=router.clone();
                        connections.spawn(async move{
                            if let Ok(Ok(tls))=tokio::time::timeout(Duration::from_secs(5),acceptor.accept(stream)).await{
                                let service=hyper_util::service::TowerToHyperService::new(router);
                                let _=hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(tls),service).await;
                            }
                        });
                    },
                    _=connections.join_next(),if !connections.is_empty()=>{},
                }
            }
        });
        Ok(Self { base, task })
    }
}
