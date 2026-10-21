$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('atelier-zip-' + [guid]::NewGuid())
[IO.Directory]::CreateDirectory($fixtureRoot) > $null
try {
    $cases = @(
        @{ Names = @('server.mjs','lib/domain/test.mjs','examples/connector/module.mjs'); Valid = $true },
        @{ Names = @('../outside'); Valid = $false },
        @{ Names = @('lib/../../outside'); Valid = $false },
        @{ Names = @('data/kanban.sqlite'); Valid = $false },
        @{ Names = @('lib/file:stream'); Valid = $false },
        @{ Names = @('lib/CON.txt'); Valid = $false },
        @{ Names = @('lib/file.'); Valid = $false },
        @{ Names = @('lib/a','lib/A'); Valid = $false }
    )
    $index = 0
    foreach ($case in $cases) {
        $archivePath = Join-Path $fixtureRoot "$index.zip"
        $destination = Join-Path $fixtureRoot "output-$index"
        $zip = [IO.Compression.ZipFile]::Open($archivePath, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($name in $case.Names) {
                $writer = [IO.StreamWriter]::new($zip.CreateEntry($name).Open())
                try { $writer.Write('test') } finally { $writer.Dispose() }
            }
        } finally { $zip.Dispose() }
        $accepted = $true
        try { & (Join-Path $PSScriptRoot '../scripts/Expand-Update.ps1') -Archive $archivePath -Destination $destination }
        catch { $accepted = $false }
        if ($accepted -ne $case.Valid) { throw "Archive case ${index}: unexpected acceptance=$accepted" }
        if (!$case.Valid -and (Test-Path -LiteralPath $destination)) { throw 'An invalid archive was partially extracted.' }
        $index++
    }
    Write-Output "$index archive validation cases passed."
} finally {
    $resolved = [IO.Path]::GetFullPath($fixtureRoot)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (!$resolved.StartsWith($expectedParent, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolved) -notlike 'atelier-zip-*') { throw 'Unsafe fixture cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
