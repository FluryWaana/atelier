use crate::Result;
use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    Storage::FileSystem::GetFullPathNameW,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_SZ,
        RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW, RegOpenKeyExW,
        RegSetValueExW,
    },
};

const REGISTRATION: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{B1FD8B3A-76DE-4DF8-96B2-502E5F10C99F}_is1";

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // This handle is owned by Key and is never closed elsewhere.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn wide(value: &std::ffi::OsStr) -> Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err("Valeur Windows invalide.".into());
    }
    value.push(0);
    Ok(value)
}

fn checked(status: u32) -> Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32).into())
    }
}

fn open(name: &str) -> Result<Option<Key>> {
    let name = wide(name.as_ref())?;
    let mut handle = null_mut();
    // The UTF-16 name and output handle remain valid throughout the native call.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            name.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY,
            &mut handle,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    checked(status)?;
    Ok(Some(Key(handle)))
}

fn read(key: &Key, name: &str) -> Result<Option<String>> {
    let name = wide(name.as_ref())?;
    let mut buffer = vec![0u16; 32_768];
    let mut bytes = u32::try_from(buffer.len() * 2)?;
    // RegGetValue bounds the buffer, expands REG_EXPAND_SZ and guarantees a terminating NUL.
    let status = unsafe {
        RegGetValueW(
            key.0,
            null(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    checked(status)?;
    if bytes as usize > buffer.len() * 2 || !bytes.is_multiple_of(2) {
        return Err("Valeur de registre invalide.".into());
    }
    buffer.truncate(bytes as usize / 2);
    // Expanded values can report their original allocation size; only the terminated string is data.
    buffer.truncate(
        buffer
            .iter()
            .position(|unit| *unit == 0)
            .ok_or("Valeur de registre non terminée.")?,
    );
    Ok(Some(String::from_utf16(&buffer)?))
}

fn write(key: &Key, name: &str, value: &str) -> Result<()> {
    let name = wide(name.as_ref())?;
    let value = wide(value.as_ref())?;
    // RegSetValue copies the owned UTF-16 buffers, including the terminating NUL.
    checked(unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            u32::try_from(value.len() * 2)?,
        )
    })
}

fn full_path(path: &Path) -> Result<Vec<u16>> {
    let path = wide(path.as_os_str())?;
    let mut buffer = vec![0u16; 32_768];
    // Both buffers are owned, and the output capacity is passed in UTF-16 code units.
    let length = unsafe {
        GetFullPathNameW(
            path.as_ptr(),
            buffer.len() as u32,
            buffer.as_mut_ptr(),
            null_mut(),
        )
    } as usize;
    if length == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if length >= buffer.len() {
        return Err("Chemin Windows trop long.".into());
    }
    buffer.truncate(length);
    while buffer.last() == Some(&(b'\\' as u16)) {
        buffer.pop();
    }
    Ok(buffer)
}

fn update_registration(root: &Path, version: &str, registration: &str) -> Result<()> {
    let Some(key) = open(registration)? else {
        return Ok(());
    };
    let Some(location) = read(&key, "InstallLocation")?.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let installed = full_path(Path::new(&location))?;
    let running = full_path(root)?;
    // Ordinal comparison uses Windows case tables, matching the existing .NET implementation.
    let compared = unsafe {
        CompareStringOrdinal(
            installed.as_ptr(),
            i32::try_from(installed.len())?,
            running.as_ptr(),
            i32::try_from(running.len())?,
            1,
        )
    };
    if compared == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if compared == CSTR_EQUAL {
        write(&key, "DisplayVersion", version)?;
    }
    Ok(())
}

pub(super) fn update(root: &Path, version: &str) -> Result<()> {
    update_registration(root, version, REGISTRATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::{RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW};

    struct TestRegistration(String);
    impl Drop for TestRegistration {
        fn drop(&mut self) {
            let name = wide(self.0.as_ref()).unwrap();
            // Only the unique disposable test subtree is removed.
            unsafe {
                RegDeleteTreeW(HKEY_CURRENT_USER, name.as_ptr());
            }
        }
    }

    #[test]
    fn registration_updates_only_the_matching_installation_and_supports_rollback() {
        let root = tempfile::tempdir().unwrap();
        let registration = TestRegistration(format!(
            r"Software\AtelierKanbanTests\{}",
            uuid::Uuid::new_v4()
        ));
        update_registration(root.path(), "1.2.3", &registration.0).unwrap();
        assert!(open(&registration.0).unwrap().is_none());
        let name = wide(registration.0.as_ref()).unwrap();
        let mut handle = null_mut();
        // The output handle is transferred immediately into its RAII owner.
        checked(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                name.as_ptr(),
                0,
                null(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY,
                null(),
                &mut handle,
                null_mut(),
            )
        })
        .unwrap();
        let key = Key(handle);
        write(
            &key,
            "InstallLocation",
            &format!("{}-other", root.path().display()),
        )
        .unwrap();
        write(&key, "DisplayVersion", "1.0.0").unwrap();
        update_registration(root.path(), "1.2.3", &registration.0).unwrap();
        assert_eq!(
            read(&key, "DisplayVersion").unwrap().as_deref(),
            Some("1.0.0")
        );
        write(
            &key,
            "InstallLocation",
            &format!("{}\\", root.path().display().to_string().to_uppercase()),
        )
        .unwrap();
        for version in ["1.2.3", "1.0.0", "literal-$()-'é"] {
            update_registration(root.path(), version, &registration.0).unwrap();
            assert_eq!(
                read(&key, "DisplayVersion").unwrap().as_deref(),
                Some(version)
            );
        }
        let location = wide("InstallLocation".as_ref()).unwrap();
        // This test owns the key and deletes only its own location value.
        checked(unsafe { RegDeleteValueW(key.0, location.as_ptr()) }).unwrap();
        update_registration(root.path(), "2.0.0", &registration.0).unwrap();
        assert_eq!(
            read(&key, "DisplayVersion").unwrap().as_deref(),
            Some("literal-$()-'é")
        );
        assert!(write(&key, "DisplayVersion", "bad\0value").is_err());
        let expanded = wide(r"%SystemRoot%\AtelierSynthetic".as_ref()).unwrap();
        // Expansion is exercised only in the disposable key; no installation directory is written.
        checked(unsafe {
            RegSetValueExW(
                key.0,
                location.as_ptr(),
                0,
                windows_sys::Win32::System::Registry::REG_EXPAND_SZ,
                expanded.as_ptr().cast(),
                (expanded.len() * 2) as u32,
            )
        })
        .unwrap();
        let expanded_root = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("AtelierSynthetic");
        update_registration(&expanded_root, "3.0.0", &registration.0).unwrap();
        assert_eq!(
            read(&key, "DisplayVersion").unwrap().as_deref(),
            Some("3.0.0")
        );
    }
}
