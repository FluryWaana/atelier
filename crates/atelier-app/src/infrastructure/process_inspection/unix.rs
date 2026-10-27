use crate::Result;
use atelier_core::contracts::ServiceEndpoint;
use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::net::{Ipv4Addr, Ipv6Addr};

#[cfg(target_os = "linux")]
pub(super) async fn listeners(owned: &BTreeSet<u32>) -> Result<Vec<ServiceEndpoint>> {
    let owned = owned.clone();
    tokio::task::spawn_blocking(move || linux_listeners(&owned)).await?
}

#[cfg(target_os = "linux")]
fn linux_listeners(owned: &BTreeSet<u32>) -> Result<Vec<ServiceEndpoint>> {
    use std::{collections::BTreeSet, io::Read};
    if owned.len() > 4096 {
        return Err("Trop de processus à inspecter.".into());
    }
    let mut endpoints = Vec::new();
    let mut descriptors = 0usize;
    let mut bytes = 0usize;
    for &pid in owned {
        let directory = match std::fs::read_dir(format!("/proc/{pid}/fd")) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let mut sockets = BTreeSet::new();
        for entry in directory {
            descriptors += 1;
            if descriptors > 65_536 {
                return Err("Trop de descripteurs à inspecter.".into());
            }
            let entry = entry?;
            match std::fs::read_link(entry.path()) {
                Ok(link) => {
                    if let Some(inode) = link
                        .to_str()
                        .and_then(|link| link.strip_prefix("socket:["))
                        .and_then(|link| link.strip_suffix(']'))
                    {
                        sockets.insert(inode.parse::<u64>()?);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        if sockets.is_empty() {
            continue;
        }
        for (name, ipv6) in [("tcp", false), ("tcp6", true)] {
            let mut table = Vec::new();
            let file = match std::fs::File::open(format!("/proc/{pid}/net/{name}")) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            file.take(1024 * 1024 + 1).read_to_end(&mut table)?;
            bytes = bytes
                .checked_add(table.len())
                .ok_or("Inspection excessive.")?;
            if table.len() > 1024 * 1024 || bytes > 16 * 1024 * 1024 {
                return Err("Tables de ports trop volumineuses.".into());
            }
            for line in std::str::from_utf8(&table)?.lines().skip(1) {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if fields.len() < 10 {
                    return Err("Table de ports invalide.".into());
                }
                if fields[3] != "0A" || !sockets.contains(&fields[9].parse::<u64>()?) {
                    continue;
                }
                let (address, port) = linux_address(fields[1], ipv6)?;
                if port != 0 {
                    endpoints.push(ServiceEndpoint {
                        pid,
                        address,
                        port,
                        url: None,
                    });
                }
                if endpoints.len() > 256 {
                    return Err("Trop de ports ouverts pour ce service (256 maximum).".into());
                }
            }
        }
    }
    endpoints.sort_by(|a, b| (a.pid, &a.address, a.port).cmp(&(b.pid, &b.address, b.port)));
    endpoints.dedup();
    Ok(endpoints)
}

#[cfg(target_os = "linux")]
fn linux_address(value: &str, ipv6: bool) -> Result<(String, u16)> {
    let (address, port) = value.split_once(':').ok_or("Adresse de port invalide.")?;
    if port.len() != 4 {
        return Err("Port hexadécimal invalide.".into());
    }
    let port = u16::from_str_radix(port, 16)?;
    let address = if ipv6 {
        if address.len() != 32 || !address.is_ascii() {
            return Err("Adresse IPv6 invalide.".into());
        }
        let mut octets = [0; 16];
        for (index, word) in octets.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            word.copy_from_slice(
                &u32::from_str_radix(&address[index * 8..index * 8 + 8], 16)?.to_ne_bytes(),
            );
        }
        Ipv6Addr::from(octets).to_string()
    } else {
        if address.len() != 8 {
            return Err("Adresse IPv4 invalide.".into());
        }
        Ipv4Addr::from(u32::from_str_radix(address, 16)?.to_ne_bytes()).to_string()
    };
    Ok((address, port))
}

#[cfg(target_os = "macos")]
pub(super) async fn listeners(owned: &BTreeSet<u32>) -> Result<Vec<ServiceEndpoint>> {
    use std::time::Duration;
    if owned.is_empty() {
        return Ok(Vec::new());
    }
    if owned.len() > 4096 {
        return Err("Trop de processus à inspecter.".into());
    }
    let selected = owned
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut command = tokio::process::Command::new("/usr/sbin/lsof");
    command
        .args([
            "-nP",
            "-a",
            "-p",
            &selected,
            "-iTCP",
            "-sTCP:LISTEN",
            "-Fpftn",
        ])
        .env_clear()
        .env("LC_ALL", "C");
    let output =
        super::super::process_tree::capture(&mut command, &[], Duration::from_secs(2), 1024 * 1024)
            .await?;
    if output.status.code() == Some(1) && output.stdout.is_empty() {
        return Ok(Vec::new());
    }
    if !output.status.success() {
        return Err("Inspection native des ports indisponible.".into());
    }
    macos_fields(&output.stdout, owned)
}

#[cfg(any(target_os = "macos", test))]
fn macos_fields(bytes: &[u8], owned: &BTreeSet<u32>) -> Result<Vec<ServiceEndpoint>> {
    let mut endpoints = Vec::new();
    let mut pid = None;
    let mut ipv6 = None;
    for (index, line) in std::str::from_utf8(bytes)?.lines().enumerate() {
        if index >= 100_000 {
            return Err("Réponse de ports excessive.".into());
        }
        let Some((kind, value)) = line.as_bytes().split_first() else {
            continue;
        };
        let value = std::str::from_utf8(value)?;
        match kind {
            b'p' => {
                let owner = value.parse::<u32>()?;
                if !owned.contains(&owner) {
                    return Err("Port attribué à un processus étranger.".into());
                }
                pid = Some(owner);
                ipv6 = None;
            }
            b'f' => {
                let _: u32 = value.parse()?;
                ipv6 = None;
            }
            b't' => match value {
                "IPv4" => ipv6 = Some(false),
                "IPv6" => ipv6 = Some(true),
                _ => return Err("Type de socket inattendu.".into()),
            },
            b'n' => {
                let (host, port) = value
                    .rsplit_once(':')
                    .ok_or("Adresse de socket invalide.")?;
                let ipv6 = ipv6.ok_or("Famille de socket absente.")?;
                let address = if host == "*" {
                    if ipv6 { "::".into() } else { "0.0.0.0".into() }
                } else {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()?
                        .to_string()
                };
                let port = port.parse::<u16>()?;
                if port != 0 {
                    endpoints.push(ServiceEndpoint {
                        pid: pid.ok_or("Processus du port absent.")?,
                        address,
                        port,
                        url: None,
                    });
                }
                if endpoints.len() > 256 {
                    return Err("Trop de ports ouverts pour ce service (256 maximum).".into());
                }
            }
            _ => return Err("Champ d’inspection de port inattendu.".into()),
        }
    }
    endpoints.sort_by(|a, b| (a.pid, &a.address, a.port).cmp(&(b.pid, &b.address, b.port)));
    endpoints.dedup();
    Ok(endpoints)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_fields_never_attribute_foreign_owners_or_guess_invalid_addresses() {
        let owned = BTreeSet::from([42]);
        let ports =
            macos_fields(b"p42\ntIPv4\nn127.0.0.1:4317\ntIPv6\nn[::1]:4318\n", &owned).unwrap();
        assert_eq!(ports[0].address, "127.0.0.1");
        assert_eq!(ports[1].address, "::1");
        assert!(macos_fields(b"p43\ntIPv4\nn127.0.0.1:4317\n", &owned).is_err());
        assert!(macos_fields(b"n127.0.0.1:4317\n", &owned).is_err());
        assert!(macos_fields(b"p42\ntIPv4\nnforeign.invalid:4317\n", &owned).is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn kernel_hex_addresses_preserve_native_word_order_and_both_families() {
        assert_eq!(
            linux_address("0100007F:10DD", false).unwrap(),
            ("127.0.0.1".into(), 4317)
        );
        assert_eq!(
            linux_address("00000000000000000000000001000000:10DE", true).unwrap(),
            ("::1".into(), 4318)
        );
        assert!(linux_address("127.0.0.1:4317", false).is_err());
        assert!(linux_address("0100007F:10000", false).is_err());
    }
}
