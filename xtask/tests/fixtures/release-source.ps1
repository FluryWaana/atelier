param([string]$Helper)
$ErrorActionPreference = 'Stop'
$ast = [Management.Automation.Language.Parser]::ParseFile($Helper, [ref]$null, [ref]$null)
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-QualifiedReleaseSource' }, $true)
. ([scriptblock]::Create($function.Extent.Text))
$commit = 'a' * 40
foreach ($scenario in @('valid', 'wrong-commit', 'fork', 'branch', 'workflow', 'event', 'failed-run', 'running', 'missing-job', 'skipped-job', 'duplicate-job', 'wrong-id', 'invalid-id')) {
  $run = [pscustomobject]@{ id=123; head_sha=$commit; head_branch='main'; head_repository=[pscustomobject]@{full_name='FluryWaana/atelier'}; path='.github/workflows/rust.yml'; event='workflow_dispatch'; status='completed'; conclusion='success' }
  $jobs = @('selection', 'checks', 'ui-payload', 'browser-build (windows-latest)', 'native (windows-latest)', 'delivery (windows-latest)', 'browser (windows-latest)') | ForEach-Object { [pscustomobject]@{name=$_; status='completed'; conclusion='success'} }
  $id = '123'
  switch ($scenario) {
    'wrong-commit' { $run.head_sha = 'b' * 40 }
    'fork' { $run.head_repository.full_name = 'other/atelier' }
    'branch' { $run.head_branch = 'other' }
    'workflow' { $run.path = '.github/workflows/other.yml' }
    'event' { $run.event = 'pull_request' }
    'failed-run' { $run.conclusion = 'failure' }
    'running' { $run.status = 'in_progress' }
    'missing-job' { $jobs = $jobs | Where-Object { $_.name -ne 'checks' } }
    'skipped-job' { $jobs[1].conclusion = 'skipped' }
    'duplicate-job' { $jobs += $jobs[1] }
    'wrong-id' { $id = '124' }
    'invalid-id' { $id = '../123' }
  }
  $refused = $false
  try { Assert-QualifiedReleaseSource $run $jobs $commit $id } catch { $refused = $true }
  if ($refused -ne ($scenario -ne 'valid')) { throw "Release source gate failed: $scenario" }
}
Write-Output 'Release source provenance contract passed.'
