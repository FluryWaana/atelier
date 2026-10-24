use atelier_app::infrastructure::session;
use serde_json::{Value, json};
use std::{
    fs, io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
};
use windows_sys::Win32::{
    Security::{GetLengthSid, GetTokenInformation, IsValidSid, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const PLAIN: &str = "atelier-m02-synthetic-account-é-🔑";
pub const MARKER: &str = "atelier-m02-synthetic-keyring";

fn account() -> Result<String> {
    let mut raw = std::ptr::null_mut();
    // OpenProcessToken returns an owned handle for this process, not an environment identity.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut size,
        )
    };
    if usize::try_from(size)? < std::mem::size_of::<TOKEN_USER>() {
        return Err("Missing token user information".into());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    // The aligned buffer has the requested size; its SID remains borrowed until copied.
    unsafe {
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        if IsValidSid(user.User.Sid) == 0 {
            return Err("Invalid process SID".into());
        }
        Ok(session::hexadecimal(std::slice::from_raw_parts(
            user.User.Sid.cast(),
            GetLengthSid(user.User.Sid) as usize,
        )))
    }
}

fn own_record() -> Result<Value> {
    let encrypted = session::protect(PLAIN, false).map_err(|_| "Synthetic encryption failed")?;
    if session::protect(&encrypted, true).map_err(|_| "Synthetic decryption failed")? != PLAIN {
        return Err("Same-account positive control failed".into());
    }
    Ok(json!({"account": account()?, "encrypted": encrypted}))
}

fn read(directory: &Path, name: &str) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(directory.join(name))?)?)
}

fn reject(record: &Value) -> Result<()> {
    if record["account"] == account()? {
        return Err("A different OS account is required".into());
    }
    let encrypted = record["encrypted"]
        .as_str()
        .ok_or("Missing synthetic ciphertext")?;
    if session::protect(encrypted, true).is_ok() {
        return Err("Another account decrypted the synthetic secret".into());
    }
    Ok(())
}

pub fn probe(directory: &Path, phase: &str) -> Result<()> {
    if fs::read_to_string(directory.join(".keyring-fixture"))? != MARKER {
        return Err("Not a synthetic keyring fixture".into());
    }
    match phase {
        "prepare" => fs::write(
            directory.join("first.json"),
            serde_json::to_vec(&own_record()?)?,
        )?,
        "other" => {
            let own = own_record()?;
            reject(&read(directory, "first.json")?)?;
            fs::write(directory.join("other.json"), serde_json::to_vec(&own)?)?;
        }
        "verify" => {
            reject(&read(directory, "other.json")?)?;
            let original = read(directory, "first.json")?;
            if original["account"] != account()?
                || session::protect(
                    original["encrypted"]
                        .as_str()
                        .ok_or("Missing synthetic ciphertext")?,
                    true,
                )
                .map_err(|_| "Original synthetic secret lost")?
                    != PLAIN
            {
                return Err("Original account did not retain its secret".into());
            }
            println!(
                "DPAPI: distinct native accounts, positive controls and reciprocal refusal verified."
            );
        }
        _ => return Err("Unknown keyring fixture phase".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_account_cannot_be_faked_by_running_the_probe_twice() {
        let directory = tempfile::tempdir().unwrap();
        assert!(probe(directory.path(), "prepare").is_err());
        fs::write(directory.path().join(".keyring-fixture"), MARKER).unwrap();
        probe(directory.path(), "prepare").unwrap();
        let before = fs::read(directory.path().join("first.json")).unwrap();
        assert!(probe(directory.path(), "other").is_err());
        assert!(!directory.path().join("other.json").exists());
        assert_eq!(
            fs::read(directory.path().join("first.json")).unwrap(),
            before
        );
    }
}
