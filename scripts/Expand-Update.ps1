param([Parameter(Mandatory)][string]$Archive, [Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$targetRoot = [IO.Path]::GetFullPath($Destination).TrimEnd('\') + '\'
if (Test-Path -LiteralPath $Destination) { throw 'Le dossier de destination existe déjà.' }
$zip = [IO.Compression.ZipFile]::OpenRead($Archive)
try {
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    [long]$total = 0
    foreach ($entry in $zip.Entries) {
        $name = $entry.FullName.Replace('\', '/')
        $parts = $name.TrimEnd('/').Split('/')
        if ($parts.Count -eq 0 -or $parts[0] -notin @('server.mjs','launcher.mjs','package.json','release.json','Start.cmd','Stop.cmd', 'Start.command', 'Stop.command','README.md','lib','public','runtime','scripts','docs','examples')) { throw 'Contenu du package interdit.' }
        foreach ($part in $parts) {
            if (!$part -or $part -in @('.','..') -or $part -match '[<>:"|?*\x00-\x1f]' -or $part -match '[. ]$' -or $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') { throw 'Chemin du package invalide.' }
        }
        $target = [IO.Path]::GetFullPath([IO.Path]::Combine($targetRoot, $name))
        if (!$target.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase) -or !$names.Add($name.TrimEnd('/'))) { throw 'Chemin du package dupliqué ou hors destination.' }
        if ((($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'Lien symbolique interdit.' }
        $total += $entry.Length
        if ($total -gt 536870912 -or $names.Count -gt 10000) { throw 'Package trop volumineux.' }
    }
} finally { $zip.Dispose() }
[IO.Compression.ZipFile]::ExtractToDirectory($Archive, $Destination)
