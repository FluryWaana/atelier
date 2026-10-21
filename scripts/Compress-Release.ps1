param([Parameter(Mandatory)][string]$Source, [Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Get-ChildItem -LiteralPath $Source -Recurse -Force | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'Un lien symbolique ne peut pas être publié.' }
[IO.Compression.ZipFile]::CreateFromDirectory($Source, $Destination, [IO.Compression.CompressionLevel]::Optimal, $false)
