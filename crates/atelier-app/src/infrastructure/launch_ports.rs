use crate::Result;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::net::{TcpListener, TcpSocket};

pub struct PortLease {
    listener: TcpListener,
    pub port: u16,
}

impl PortLease {
    pub fn reserve(port: u16) -> Result<Self> {
        Self::bind(port).map_err(|_| {
            if port == 0 {
                "Impossible de réserver un port local.".into()
            } else {
                format!("Le port {port} est indisponible. Choisissez un autre port ou laissez Atelier le choisir.").into()
            }
        })
    }

    fn bind(port: u16) -> std::io::Result<Self> {
        let socket = TcpSocket::new_v4()?;
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawSocket;
            use windows_sys::Win32::Networking::WinSock::{
                SO_EXCLUSIVEADDRUSE, SOCKET_ERROR, SOL_SOCKET, WSAGetLastError, setsockopt,
            };
            let enabled = 1i32;
            // A same-account SO_REUSEADDR socket must not steal a reserved Windows port.
            if unsafe {
                setsockopt(
                    socket.as_raw_socket() as usize,
                    SOL_SOCKET,
                    SO_EXCLUSIVEADDRUSE,
                    (&enabled as *const i32).cast(),
                    std::mem::size_of_val(&enabled) as i32,
                )
            } == SOCKET_ERROR
            {
                return Err(std::io::Error::from_raw_os_error(unsafe {
                    WSAGetLastError()
                }));
            }
        }
        socket.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        let listener = socket.listen(511)?;
        Ok(Self {
            port: listener.local_addr()?.port(),
            listener,
        })
    }

    pub fn release(self) {
        drop(self.listener);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reservations_are_loopback_unique_exclusive_and_released_on_drop() {
        let mut leases = Vec::new();
        for _ in 0..16 {
            let lease = PortLease::reserve(0).unwrap();
            assert_ne!(lease.port, 0);
            assert_eq!(
                lease.listener.local_addr().unwrap().ip(),
                Ipv4Addr::LOCALHOST
            );
            assert!(
                leases
                    .iter()
                    .all(|other: &PortLease| other.port != lease.port)
            );
            leases.push(lease);
        }
        let port = leases[0].port;
        let error = PortLease::reserve(port)
            .err()
            .expect("Occupied port accepted");
        assert_eq!(
            error.to_string(),
            format!(
                "Le port {port} est indisponible. Choisissez un autre port ou laissez Atelier le choisir."
            )
        );
        let competing = TcpSocket::new_v4().unwrap();
        competing.set_reuseaddr(true).unwrap();
        assert!(
            competing
                .bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
                .is_err()
        );
        leases.remove(0).release();
        let replacement = PortLease::reserve(port).unwrap();
        assert_eq!(replacement.port, port);
        drop(replacement);
        let _reopened = PortLease::reserve(port).unwrap();
    }
}
