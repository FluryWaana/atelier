use std::{env, fs, path::PathBuf, process::Command};

fn numeric_version<const N: usize>(value: &str) -> Option<[u16; N]> {
    value
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn resource_script(version: &str) -> Result<String, &'static str> {
    let [major, minor, patch] = numeric_version::<3>(version)
        .ok_or("Windows resources require three u16 version numbers.")?;
    Ok(format!(
        r#"1 ICON "atelier-kanban.ico"
1 VERSIONINFO
 FILEVERSION {major},{minor},{patch},0
 PRODUCTVERSION {major},{minor},{patch},0
 FILEFLAGSMASK 0x3f
 FILEFLAGS 0
 FILEOS 0x40004
 FILETYPE 1
 FILESUBTYPE 0
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904b0"
  BEGIN
   VALUE "FileDescription", "Atelier Kanban\0"
   VALUE "FileVersion", "{version}\0"
   VALUE "InternalName", "atelier\0"
   VALUE "OriginalFilename", "atelier.exe\0"
   VALUE "ProductName", "Atelier Kanban\0"
   VALUE "ProductVersion", "{version}\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x0409, 1200
 END
END
"#
    ))
}

fn resource_compiler() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let sdk_roots = [
        env::var_os("WindowsSdkDir").map(PathBuf::from),
        env::var_os("ProgramFiles(x86)").map(|root| PathBuf::from(root).join("Windows Kits/10")),
    ];
    for sdk in sdk_roots.into_iter().flatten() {
        let bin = sdk.join("bin");
        let entries = match fs::read_dir(&bin) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry?;
            if let Some(version) = numeric_version::<4>(&entry.file_name().to_string_lossy()) {
                let compiler = entry.path().join("x64/rc.exe");
                if compiler.is_file() {
                    candidates.push((version, compiler));
                }
            }
        }
        if let Some((_, compiler)) = candidates.into_iter().max_by_key(|(version, _)| *version) {
            return Ok(compiler);
        }
        let compiler = bin.join("x64/rc.exe");
        if compiler.is_file() {
            return Ok(compiler);
        }
    }
    Err("Windows SDK x64 rc.exe is required to build Atelier Kanban Windows resources.".into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources/atelier-kanban.ico");
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=ProgramFiles(x86)");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return Ok(());
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo output directory missing.")?);
    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").ok_or("Cargo manifest directory missing.")?,
    );
    let script = resource_script(&env::var("CARGO_PKG_VERSION")?)?;
    fs::copy(
        manifest.join("resources/atelier-kanban.ico"),
        output.join("atelier-kanban.ico"),
    )?;
    fs::write(output.join("atelier.rc"), script)?;
    let mut compiler = Command::new(resource_compiler()?);
    compiler
        .current_dir(&output)
        .args(["/nologo", "/fo", "atelier.res", "atelier.rc"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        compiler.creation_flags(0x08000000);
    }
    if !compiler.status()?.success() {
        return Err("Windows resource compilation failed.".into());
    }
    println!(
        "cargo:rustc-link-arg-bin=atelier={}",
        output.join("atelier.res").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_identifies_the_application_and_preserves_numeric_version_components() {
        let script = resource_script("0.1.10").unwrap();
        for expected in [
            "1 ICON \"atelier-kanban.ico\"",
            "FILEVERSION 0,1,10,0",
            "PRODUCTVERSION 0,1,10,0",
            "VALUE \"FileDescription\", \"Atelier Kanban\\0\"",
            "VALUE \"ProductName\", \"Atelier Kanban\\0\"",
            "VALUE \"OriginalFilename\", \"atelier.exe\\0\"",
            "VALUE \"FileVersion\", \"0.1.10\\0\"",
        ] {
            assert!(
                script.contains(expected),
                "Missing resource field: {expected}"
            );
        }
        for invalid in [
            "1.2",
            "1.2.3.4",
            "1.2.3-beta",
            "1.2.65536",
            "1.+2.3",
            "1.2.3\n",
        ] {
            assert!(
                resource_script(invalid).is_err(),
                "Accepted invalid version: {invalid}"
            );
        }
        assert!(resource_script("65535.65535.65535").is_ok());
    }

    #[test]
    fn sdk_versions_sort_numerically_and_ignore_non_version_directories() {
        assert!(numeric_version::<4>("10.0.26100.0") > numeric_version::<4>("10.0.9999.0"));
        assert_eq!(numeric_version::<4>("x64"), None);
        assert_eq!(numeric_version::<4>("10.0.partial.0"), None);
    }
}
