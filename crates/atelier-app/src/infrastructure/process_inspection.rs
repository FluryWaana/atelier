use crate::Result;
use atelier_core::contracts::ServiceEndpoint;
#[cfg(any(windows, test))]
use std::net::Ipv4Addr;
use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv6Addr},
    sync::LazyLock,
};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    },
    Networking::WinSock::{AF_INET, AF_INET6},
};

#[cfg(windows)]
const TABLE_LIMIT: usize = 16 * 1024 * 1024;
static URL: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"https?://[^\s<>"'()]+"#).expect("constant URL expression")
});

#[cfg(windows)]
fn table(family: u16) -> Result<(Vec<u64>, usize)> {
    let mut size = 8192u32;
    for _ in 0..6 {
        if size < 4 || size as usize > TABLE_LIMIT {
            return Err("Table des ports trop volumineuse.".into());
        }
        let mut data = vec![0u64; (size as usize).div_ceil(8)];
        // Windows writes only within this aligned, initialized allocation and reports the required size.
        let status = unsafe {
            GetExtendedTcpTable(
                data.as_mut_ptr().cast(),
                &mut size,
                0,
                u32::from(family),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32).into());
        }
        if size < 4 || size as usize > data.len() * 8 {
            return Err("Table des ports invalide.".into());
        }
        return Ok((data, size as usize));
    }
    Err("La table des ports change trop rapidement. Réessaie.".into())
}

#[cfg(windows)]
fn rows<T: Copy>(data: &[u64], size: usize, offset: usize) -> Result<Vec<T>> {
    if size < 4 || size > std::mem::size_of_val(data) || offset < 4 || offset > size {
        return Err("Table des ports invalide.".into());
    }
    let bytes = data.as_ptr().cast::<u8>();
    // The header is initialized; its count is checked against both the row size and buffer length.
    let count = unsafe { bytes.cast::<u32>().read_unaligned() } as usize;
    if count > 50_000 || count > (size - offset) / std::mem::size_of::<T>() {
        return Err("Table des ports invalide.".into());
    }
    Ok((0..count)
        .map(|index| unsafe {
            bytes
                .add(offset + index * std::mem::size_of::<T>())
                .cast::<T>()
                .read_unaligned()
        })
        .collect())
}

#[cfg(windows)]
fn windows_listeners(owned: &BTreeSet<u32>) -> Result<Vec<ServiceEndpoint>> {
    if owned.is_empty() {
        return Ok(Vec::new());
    }
    let mut endpoints = Vec::new();
    let (table4, size4) = table(AF_INET)?;
    for row in rows::<MIB_TCPROW_OWNER_PID>(
        &table4,
        size4,
        std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table),
    )? {
        if owned.contains(&row.dwOwningPid) {
            endpoints.push(ServiceEndpoint {
                pid: row.dwOwningPid,
                address: Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).to_string(),
                port: u16::from_be(row.dwLocalPort as u16),
                url: None,
            });
        }
    }
    let (table6, size6) = table(AF_INET6)?;
    for row in rows::<MIB_TCP6ROW_OWNER_PID>(
        &table6,
        size6,
        std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table),
    )? {
        if owned.contains(&row.dwOwningPid) {
            let address = Ipv6Addr::from(row.ucLocalAddr);
            endpoints.push(ServiceEndpoint {
                pid: row.dwOwningPid,
                address: if row.dwLocalScopeId != 0 {
                    format!("{address}%{}", row.dwLocalScopeId)
                } else {
                    address.to_string()
                },
                port: u16::from_be(row.dwLocalPort as u16),
                url: None,
            });
        }
    }
    endpoints.retain(|endpoint| endpoint.port != 0);
    endpoints.sort_by(|a, b| (a.pid, &a.address, a.port).cmp(&(b.pid, &b.address, b.port)));
    endpoints.dedup();
    if endpoints.len() > 256 {
        return Err("Trop de ports ouverts pour ce service (256 maximum).".into());
    }
    Ok(endpoints)
}

pub(super) async fn listeners(owned: &BTreeSet<u32>) -> Result<Vec<ServiceEndpoint>> {
    #[cfg(windows)]
    {
        let owned = owned.clone();
        tokio::task::spawn_blocking(move || windows_listeners(&owned)).await?
    }
    #[cfg(unix)]
    unix::listeners(owned).await
}

#[cfg(unix)]
mod unix;

pub(super) fn local_url(address: &str, port: u16, log: &str) -> Option<String> {
    let ip: IpAddr = address.parse().ok()?;
    if !ip.is_loopback() && !ip.is_unspecified() {
        return None;
    }
    let ip = if ip.is_unspecified() {
        if ip.is_ipv4() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        }
    } else {
        ip
    };
    let host = match ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };
    for found in URL.find_iter(log) {
        let Ok(url) = reqwest::Url::parse(found.as_str()) else {
            continue;
        };
        if url.port_or_known_default() != Some(port)
            || !url.username().is_empty()
            || url.password().is_some()
        {
            continue;
        }
        let logged = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
        if logged != "localhost" && logged != address && logged != ip.to_string() {
            continue;
        }
        return Some(format!(
            "{}://{host}:{port}{}{}{}",
            url.scheme(),
            url.path(),
            url.query()
                .map(|query| format!("?{query}"))
                .unwrap_or_default(),
            url.fragment()
                .map(|fragment| format!("#{fragment}"))
                .unwrap_or_default()
        ));
    }
    Some(format!("http://{host}:{port}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_only_link_to_local_addresses_and_reject_log_credentials() {
        assert_eq!(
            local_url("0.0.0.0", 5173, "https://localhost:5173/app?test=1#view").as_deref(),
            Some("https://127.0.0.1:5173/app?test=1#view")
        );
        assert_eq!(
            local_url("::", 443, "").as_deref(),
            Some("http://[::1]:443/")
        );
        assert_eq!(
            local_url(
                "127.0.0.1",
                4318,
                "https://user:pass@localhost:4318/private https://localhost:43180/wrong"
            )
            .as_deref(),
            Some("http://127.0.0.1:4318/")
        );
        assert!(local_url("192.0.2.1", 80, "http://192.0.2.1/").is_none());
        assert!(local_url("::1%1", 80, "").is_none());
        #[cfg(windows)]
        {
            assert!(rows::<MIB_TCPROW_OWNER_PID>(&[u64::MAX], 8, 4).is_err());
            assert!(rows::<MIB_TCPROW_OWNER_PID>(&[0], 9, 4).is_err());
        }
    }
    #[tokio::test]
    async fn native_table_finds_only_the_synthetic_owner_and_open_port() {
        let socket = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = socket.local_addr().unwrap().port();
        let owned = BTreeSet::from([std::process::id()]);
        let found = listeners(&owned).await.unwrap();
        assert!(
            found
                .iter()
                .all(|endpoint| endpoint.pid == std::process::id())
        );
        assert!(
            found
                .iter()
                .any(|endpoint| endpoint.port == port && endpoint.address == "127.0.0.1")
        );
        assert!(listeners(&BTreeSet::new()).await.unwrap().is_empty());
    }
}
