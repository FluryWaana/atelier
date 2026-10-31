param([Parameter(Mandatory = $true)][string]$SourceRun, [Parameter(Mandatory = $true)][string]$Commit)
$ErrorActionPreference = 'Stop'
function Assert-QualifiedReleaseSource($Run, $Jobs, [string]$Commit, [string]$SourceRun) {
  if ($SourceRun -notmatch '^[1-9][0-9]{0,19}$' -or $Commit -notmatch '^[0-9a-f]{40}$') { throw 'Invalid release source identity.' }
  if ([string]$Run.id -ne $SourceRun -or $Run.head_sha -cne $Commit -or $Run.head_branch -cne 'main' -or $Run.head_repository.full_name -cne 'FluryWaana/atelier' -or $Run.path -cne '.github/workflows/rust.yml' -or $Run.event -cne 'workflow_dispatch' -or $Run.status -cne 'completed' -or $Run.conclusion -cne 'success') { throw 'Release requires the successful manual Rust campaign on this exact main commit.' }
  foreach ($name in @('selection', 'checks', 'ui-payload', 'browser-build (windows-latest)', 'native (windows-latest)', 'delivery (windows-latest)', 'browser (windows-latest)')) {
    $found = @($Jobs | Where-Object { $_.name -ceq $name })
    if ($found.Count -ne 1 -or $found[0].status -cne 'completed' -or $found[0].conclusion -cne 'success') { throw "Required release qualification missing or unsuccessful: $name" }
  }
}
if ($SourceRun -notmatch '^[1-9][0-9]{0,19}$') { throw 'Invalid source run.' }
$runJson = & gh api "repos/FluryWaana/atelier/actions/runs/$SourceRun"
if ($LASTEXITCODE -ne 0) { throw 'Source campaign unavailable.' }
$jobsJson = & gh api "repos/FluryWaana/atelier/actions/runs/$SourceRun/jobs?per_page=100"
if ($LASTEXITCODE -ne 0) { throw 'Source jobs unavailable.' }
$jobs = $jobsJson | ConvertFrom-Json
if ($jobs.total_count -gt 100) { throw 'Source job inventory exceeded its bound.' }
Assert-QualifiedReleaseSource ($runJson | ConvertFrom-Json) $jobs.jobs $Commit $SourceRun
