use super::checked_file::{CheckedFile, Identity};
use crate::Result;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use subtle::ConstantTimeEq;

pub fn hexadecimal(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn new_token() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Générateur aléatoire indisponible.")?;
    Ok(hexadecimal(&bytes))
}

pub fn same_secret(actual: &str, expected: &str) -> bool {
    actual.as_bytes().ct_eq(expected.as_bytes()).into()
}

pub fn proof(token: &str, challenge: &str, port: u16) -> Result<String> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).map_err(|_| "Clé de session invalide.")?;
    mac.update(format!("{port}:{challenge}").as_bytes());
    Ok(hexadecimal(&mac.finalize().into_bytes()))
}

#[cfg(windows)]
pub fn protect(value: &str, decrypt: bool) -> Result<String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let mut bytes: Vec<u8> = if decrypt {
        if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Coffre Windows invalide.".into());
        }
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16))
            .collect::<std::result::Result<_, _>>()?
    } else {
        value.encode_utf16().flat_map(u16::to_le_bytes).collect()
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len())?,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // DPAPI owns the output allocation; copy it before LocalFree and never retain its pointer.
    unsafe {
        let ok = if decrypt {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let data = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        if !decrypt {
            return Ok(hexadecimal(&data));
        }
        if !data.len().is_multiple_of(2) {
            return Err("Coffre Windows invalide.".into());
        }
        Ok(String::from_utf16(
            &data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )?)
    }
}

pub fn write(directory: &Path, token: &str) -> Result<()> {
    #[cfg(windows)]
    let stored = protect(token, false)?;
    #[cfg(not(windows))]
    let stored = token.to_owned();
    write_private(&directory.join("local-session"), &stored)
}

fn write_private(path: &Path, stored: &str) -> Result<()> {
    let temporary = path.with_extension(uuid::Uuid::new_v4().to_string());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(stored.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn read_optional(directory: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(directory.join("local-session")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(_) => read(directory).map(Some),
    }
}

pub fn browser_address(directory: &Path, address: &str, token: &str) -> Result<String> {
    let port = super::local_http::server_port(address)?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Session locale invalide.".into());
    }
    let address = format!("http://127.0.0.1:{port}/#session={token}");
    #[cfg(windows)]
    {
        let _ = directory;
        Ok(address)
    }
    #[cfg(unix)]
    {
        let path = directory.join("local-browser.html");
        write_private(
            &path,
            &format!(
                "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={address}\">"
            ),
        )?;
        reqwest::Url::from_file_path(&path)
            .map(|url| url.to_string())
            .map_err(|_| "Chemin du navigateur invalide.".into())
    }
}

pub fn read(directory: &Path) -> Result<String> {
    let path = directory.join("local-session");
    let expected = Identity::inspect(&path)?;
    let stored = String::from_utf8(
        CheckedFile::open(&path, &expected)?
            .private()?
            .read(65_536)?,
    )?;
    #[cfg(windows)]
    let token = protect(&stored, true)?;
    #[cfg(not(windows))]
    let token = stored;
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Session locale invalide.".into());
    }
    Ok(token)
}
