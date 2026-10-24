use crate::Result;
use cap_std::fs::Dir;
use std::path::Path;

#[cfg(unix)]
pub(super) fn create(parent: &Dir, name: &Path, target: &Path) -> Result<()> {
    parent.symlink_contents(target, name)?;
    Ok(())
}

#[cfg(windows)]
fn junction_buffer(target: &Path) -> Result<Vec<u16>> {
    let print = super::task_creation::path_string(target)
        .utf8()?
        .replace('/', "\\");
    // Mount points support local volumes; network shares require a different link mechanism.
    if print.len() < 3
        || !print.as_bytes()[0].is_ascii_alphabetic()
        || &print.as_bytes()[1..3] != b":\\"
        || print.contains('\0')
    {
        return Err("Les dépendances partagées nécessitent un disque local.".into());
    }
    let substitute = format!("\\??\\{print}").encode_utf16().collect::<Vec<_>>();
    let print = print.encode_utf16().collect::<Vec<_>>();
    let data_bytes = 8 + (substitute.len() + print.len() + 2) * 2;
    if data_bytes + 8 > 16 * 1024 {
        return Err("Chemin de dépendances trop long.".into());
    }
    // REPARSE_DATA_BUFFER: mount-point tag, header, byte offsets, then two terminated UTF-16 names.
    let mut buffer = vec![
        0x0003,
        0xa000,
        data_bytes as u16,
        0,
        0,
        (substitute.len() * 2) as u16,
        ((substitute.len() + 1) * 2) as u16,
        (print.len() * 2) as u16,
    ];
    buffer.extend(substitute);
    buffer.push(0);
    buffer.extend(print);
    buffer.push(0);
    Ok(buffer)
}

#[cfg(windows)]
pub(super) fn create(parent: &Dir, name: &Path, target: &Path) -> Result<()> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_REPARSE_POINT};
    let buffer = junction_buffer(target)?;
    parent.create_dir(name)?;
    let result = (|| -> Result<()> {
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No);
        let file = parent.open_with(name, &options)?;
        super::checked_file::require_plain(&file.metadata()?)?;
        let mut returned = 0;
        // The handle stays open and Windows reads exactly the initialized buffer during this call.
        if unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                (buffer.len() * 2) as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    })();
    if result.is_err() {
        // Nonrecursive removal can never delete the shared dependency contents.
        parent.remove_dir(name)?;
    }
    result
}

pub(super) fn remove(parent: &Dir, name: &Path) -> Result<()> {
    #[cfg(windows)]
    parent.remove_dir(name)?;
    #[cfg(unix)]
    parent.remove_file(name)?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn junction_has_byte_offsets_and_rejects_network_paths() {
        let buffer = junction_buffer(Path::new("C:/synthetic/😀")).unwrap();
        assert_eq!(&buffer[..2], &[3, 0xa000]);
        assert_eq!(usize::from(buffer[2]) + 8, buffer.len() * 2);
        let substitute_len = usize::from(buffer[5]) / 2;
        assert_eq!(
            String::from_utf16(&buffer[8..8 + substitute_len]).unwrap(),
            "\\??\\C:\\synthetic\\😀"
        );
        assert_eq!(buffer[8 + substitute_len], 0);
        assert_eq!(usize::from(buffer[6]), (substitute_len + 1) * 2);
        assert!(junction_buffer(Path::new("//server/share/modules")).is_err());
        assert!(junction_buffer(Path::new("relative/modules")).is_err());
    }

    #[test]
    fn junction_removal_preserves_source_and_existing_destination() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        root.create_dir("source").unwrap();
        root.write("source/keep", b"synthetic dependency").unwrap();
        let source = std::fs::canonicalize(fixture.path().join("source")).unwrap();
        create(&root, Path::new("modules"), &source).unwrap();
        assert_eq!(
            std::fs::read(fixture.path().join("modules/keep")).unwrap(),
            b"synthetic dependency"
        );
        assert!(create(&root, Path::new("modules"), &source).is_err());
        remove(&root, Path::new("modules")).unwrap();
        assert_eq!(root.read("source/keep").unwrap(), b"synthetic dependency");
    }
}
