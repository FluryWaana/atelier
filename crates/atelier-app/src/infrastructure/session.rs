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
    let temporary = directory.join(format!("local-session.{}", uuid::Uuid::new_v4()));
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
        fs::rename(&temporary, directory.join("local-session"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn read(directory: &Path) -> Result<String> {
    // shortcut: isolated M01 test sessions only; add descriptor identity and permission checks in M02.
    let stored = fs::read_to_string(directory.join("local-session"))?;
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
