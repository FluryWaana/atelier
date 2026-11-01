#![cfg(windows)]

#[test]
fn application_does_not_require_a_separate_visual_cpp_runtime_installation() {
    use std::os::windows::process::CommandExt;
    let native = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = std::process::Command::new(native)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(r#"$ErrorActionPreference='Stop'
$where=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$visualStudio=@(& $where -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if ($LASTEXITCODE -ne 0 -or $visualStudio.Count -ne 1) { throw 'MSVC is required for the Windows distribution check.' }
$toolset=(Get-Content -LiteralPath (Join-Path $visualStudio[0] 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
if ($toolset -notmatch '^\d+\.\d+\.\d+$') { throw 'Invalid default MSVC toolset version.' }
$dumpbin=Join-Path $visualStudio[0] "VC/Tools/MSVC/$toolset/bin/Hostx64/x64/dumpbin.exe"
if (-not (Test-Path -LiteralPath $dumpbin -PathType Leaf)) { throw 'MSVC dumpbin is required for the Windows distribution check.' }
$imports=& $dumpbin /DEPENDENTS $env:ATELIER_SYNTHETIC_BINARY
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Windows imports.' }
if (($imports -join [char]10) -match '(?im)^\s*(vcruntime|msvcp|ucrtbase|api-ms-win-crt-)[^\r\n]*\.dll\s*$') { throw 'The application still imports a separately installed C runtime.' }
"#)
        .env("ATELIER_SYNTHETIC_BINARY", env!("CARGO_BIN_EXE_atelier"))
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn application_executable_contains_the_brand_icon_and_its_real_product_version() {
    use std::os::windows::{ffi::OsStrExt, process::CommandExt};
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_atelier"));
    let wide: Vec<u16> = binary.as_os_str().encode_wide().chain(Some(0)).collect();
    // An index of -1 and zero output slots queries icon groups without creating icon handles.
    let icons = unsafe {
        windows_sys::Win32::UI::Shell::ExtractIconExW(
            wide.as_ptr(),
            -1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    assert!(
        icons > 0 && icons != u32::MAX,
        "Application icon resource missing"
    );
    let native = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = std::process::Command::new(native)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg("$v=[Diagnostics.FileVersionInfo]::GetVersionInfo($env:ATELIER_SYNTHETIC_BINARY); if ($v.ProductName -cne 'Atelier Kanban' -or $v.FileDescription -cne 'Atelier Kanban' -or $v.FileVersion -cne $env:ATELIER_EXPECTED_VERSION -or $v.ProductVersion -cne $env:ATELIER_EXPECTED_VERSION) { throw 'Native product metadata mismatch.' }")
        .env("ATELIER_SYNTHETIC_BINARY", binary)
        .env("ATELIER_EXPECTED_VERSION", env!("CARGO_PKG_VERSION"))
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
