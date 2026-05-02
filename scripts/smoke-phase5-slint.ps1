param(
  [switch]$CheckOnly,
  [switch]$Build,
  [switch]$InstallSmoke,
  [switch]$PublishDryRun,
  [switch]$Full,
  [string]$InstallRoot
)

$ErrorActionPreference = 'Stop'

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$generatedTempInstallRoot = $false
if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
  $InstallRoot = Join-Path ([System.IO.Path]::GetTempPath()) "SimpleDownloadManager-SlintPhase5-$PID"
  $generatedTempInstallRoot = $true
}
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)

if (-not ($CheckOnly -or $Build -or $InstallSmoke -or $PublishDryRun -or $Full)) {
  $CheckOnly = $true
}
if ($Full) {
  $Build = $true
  $InstallSmoke = $true
  $PublishDryRun = $true
}

$script:lastCommand = $null

function Resolve-MakensisPath {
  $command = Get-Command 'makensis' -ErrorAction SilentlyContinue
  if ($null -ne $command) {
    return $command.Source
  }

  $candidateRoots = @(
    ${env:ProgramFiles(x86)},
    $env:ProgramFiles
  ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

  foreach ($root in $candidateRoots) {
    foreach ($relativePath in @('NSIS\Bin\makensis.exe', 'NSIS\makensis.exe')) {
      $candidate = Join-Path $root $relativePath
      if (Test-Path $candidate) {
        return $candidate
      }
    }
  }

  return $null
}

function Add-ExecutableDirectoryToPath {
  param([string]$ExecutablePath)

  $directory = Split-Path -Parent $ExecutablePath
  if (($env:PATH -split ';') -notcontains $directory) {
    $env:PATH = "$directory;$env:PATH"
  }
}

function Test-TauriSigningConfiguration {
  $probeRoot = Join-Path ([System.IO.Path]::GetTempPath()) "SimpleDownloadManager-SlintSigningProbe-$PID"
  $probePath = Join-Path $probeRoot 'probe.txt'

  New-Item -ItemType Directory -Force -Path $probeRoot | Out-Null
  Set-Content -LiteralPath $probePath -Value 'signing probe'

  try {
    $tauriCli = Join-Path $workspaceRoot 'node_modules\@tauri-apps\cli\tauri.js'
    $output = & node $tauriCli signer sign $probePath 2>&1
    if ($LASTEXITCODE -eq 0) {
      return $null
    }

    $message = ($output | Out-String).Trim()
    if ([string]::IsNullOrWhiteSpace($message)) {
      return 'usable Tauri updater signing configuration'
    }

    return "usable Tauri updater signing configuration ($message)"
  } finally {
    Remove-Item -Recurse -Force -LiteralPath $probeRoot -ErrorAction SilentlyContinue
  }
}

function Import-LegacyTauriSigningEnvironment {
  $defaultSigningKeyPath = Join-Path $env:USERPROFILE '.simple-download-manager\tauri-updater.key'
  $signingKeyPath = $env:SDM_TAURI_SIGNING_PRIVATE_KEY_PATH

  if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    if ([string]::IsNullOrWhiteSpace($signingKeyPath)) {
      $signingKeyPath = $defaultSigningKeyPath
    }

    if (-not [string]::IsNullOrWhiteSpace($signingKeyPath) -and (Test-Path -LiteralPath $signingKeyPath)) {
      $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -LiteralPath $signingKeyPath -Raw
    }
  }

  if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    if (
      -not [string]::IsNullOrWhiteSpace($env:SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH) -and
      (Test-Path -LiteralPath $env:SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH)
    ) {
      $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -LiteralPath $env:SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH -Raw).TrimEnd()
    } else {
      $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
    }
  }
}

function Resolve-ExecutablePath {
  param([string]$Command)

  $resolved = Get-Command $Command -ErrorAction SilentlyContinue
  if ($null -ne $resolved -and -not [string]::IsNullOrWhiteSpace($resolved.Source)) {
    if ([System.IO.Path]::GetExtension($resolved.Source) -ieq '.ps1') {
      $cmdShim = [System.IO.Path]::ChangeExtension($resolved.Source, '.cmd')
      if (Test-Path -LiteralPath $cmdShim) {
        return $cmdShim
      }
    }

    return $resolved.Source
  }

  return $Command
}

function Invoke-Phase5Command {
  param(
    [string]$Command,
    [string[]]$Arguments,
    [string]$WorkingDirectory = $workspaceRoot
  )

  $script:lastCommand = "$Command $($Arguments -join ' ')"
  Write-Host "> $script:lastCommand"
  $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = Resolve-ExecutablePath $Command
  $startInfo.WorkingDirectory = $WorkingDirectory
  $startInfo.UseShellExecute = $false
  foreach ($argument in $Arguments) {
    [void]$startInfo.ArgumentList.Add($argument)
  }
  $process = [System.Diagnostics.Process]::Start($startInfo)
  $process.WaitForExit()
  if ($process.ExitCode -ne 0) {
    throw "$script:lastCommand exited with code $($process.ExitCode)"
  }
}

function Get-SlintArtifactPaths {
  $verifyScriptPath = Join-Path $workspaceRoot 'scripts\verify-release-slint.mjs'
  $script = @"
import { pathToFileURL } from 'node:url';

const [root, verifyScriptPath] = process.argv.slice(1);
const { slintRequiredArtifactPaths } = await import(pathToFileURL(verifyScriptPath).href);
const paths = await slintRequiredArtifactPaths({ root });
console.log(JSON.stringify({
  installerPath: paths.installerPath,
  signaturePath: paths.signaturePath,
  transitionFeedPath: paths.transitionMetadataPath,
  slintFeedPath: paths.metadataPath
}));
"@
  $json = node --input-type=module -e $script $workspaceRoot $verifyScriptPath
  if ($LASTEXITCODE -ne 0) {
    throw 'Could not resolve Slint artifact paths.'
  }
  return $json | ConvertFrom-Json
}

function Get-RegistryProbes {
  return @(
    @{
      browser = 'Chrome'
      key = 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager'
      valueMatches = $true
    },
    @{
      browser = 'Edge'
      key = 'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager'
      valueMatches = $true
    },
    @{
      browser = 'Firefox'
      key = 'HKCU:\Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager'
      valueMatches = $true
    }
  )
}

function Get-PrerequisiteGaps {
  param(
    [object]$Artifacts,
    [bool]$RequireArtifacts
  )

  $missing = New-Object System.Collections.Generic.List[string]
  if (-not (Get-Command 'cargo-packager' -ErrorAction SilentlyContinue)) {
    $missing.Add('cargo-packager (install with: cargo install cargo-packager --locked)')
  }
  $makensisPath = Resolve-MakensisPath
  if ($null -eq $makensisPath) {
    $missing.Add('makensis (install NSIS and ensure makensis.exe is on PATH)')
  } else {
    Add-ExecutableDirectoryToPath $makensisPath
  }
  Import-LegacyTauriSigningEnvironment
  if ([string]::IsNullOrWhiteSpace($env:CARGO_PACKAGER_SIGN_PRIVATE_KEY) -and [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    $missing.Add('CARGO_PACKAGER_SIGN_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY')
  } else {
    if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and -not [string]::IsNullOrWhiteSpace($env:CARGO_PACKAGER_SIGN_PRIVATE_KEY)) {
      $env:TAURI_SIGNING_PRIVATE_KEY = $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY
    }
    if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -and $null -ne $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD) {
      $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD
    }
    $signingConfigurationError = Test-TauriSigningConfiguration
    if ($null -ne $signingConfigurationError) {
      $missing.Add($signingConfigurationError)
    }
  }
  if (-not (Get-Command 'gh' -ErrorAction SilentlyContinue)) {
    $missing.Add('GitHub CLI gh (install from https://cli.github.com/ and run gh auth login)')
  }

  if ($RequireArtifacts) {
    foreach ($artifact in @(
      @{ label = 'Slint installer artifact'; path = $Artifacts.installerPath },
      @{ label = 'Slint installer signature'; path = $Artifacts.signaturePath },
      @{ label = 'Slint transition updater feed'; path = $Artifacts.transitionFeedPath },
      @{ label = 'Slint-native updater feed'; path = $Artifacts.slintFeedPath }
    )) {
      if (-not (Test-Path ([string]$artifact.path))) {
        $missing.Add("$($artifact.label) $($artifact.path)")
      }
    }
  }

  return @($missing)
}

function Write-Phase5SmokeReport {
  param([hashtable]$Report)

  $json = $Report | ConvertTo-Json -Depth 8
  $reportScriptPath = Join-Path $workspaceRoot 'scripts\slint-phase5-smoke-report.mjs'
  $json | & node $reportScriptPath '--write' '--root' $workspaceRoot
  if ($LASTEXITCODE -ne 0) {
    throw 'Could not write Phase 5 Slint smoke report.'
  }
}

function New-ReportBase {
  param(
    [string[]]$CheckedCommands,
    [object]$Artifacts
  )

  return @{
    timestamp = (Get-Date).ToUniversalTime().ToString('o')
    checkedCommands = $CheckedCommands
    artifacts = @{
      installerPath = [string]$Artifacts.installerPath
      signaturePath = [string]$Artifacts.signaturePath
      transitionFeedPath = [string]$Artifacts.transitionFeedPath
      slintFeedPath = [string]$Artifacts.slintFeedPath
    }
  }
}

$checkedCommands = New-Object System.Collections.Generic.List[string]
$checkedCommands.Add('npm run smoke:phase5:slint')
if ($Build) {
  $checkedCommands.Add('npm run release:windows:slint')
}
$checkedCommands.Add('node .\scripts\verify-release-slint.mjs')
if ($PublishDryRun) {
  $checkedCommands.Add('node .\scripts\publish-updater-alpha-slint.mjs --dry-run')
}
if ($InstallSmoke) {
  $checkedCommands.Add('pwsh -ExecutionPolicy Bypass -File ".\scripts\smoke-release-slint.ps1"')
}

$artifacts = Get-SlintArtifactPaths
$requireArtifactsBeforeRun = -not $Build
$gaps = Get-PrerequisiteGaps -Artifacts $artifacts -RequireArtifacts:$requireArtifactsBeforeRun

if ($gaps.Count -gt 0) {
  $report = New-ReportBase -CheckedCommands @($checkedCommands) -Artifacts $artifacts
  $report.status = 'blocked'
  $report.prerequisiteGaps = @($gaps)
  Write-Phase5SmokeReport -Report $report
  Write-Host "Phase 5 Slint smoke blocked:`n - $($gaps -join "`n - ")"
  if ($CheckOnly -and -not $Build -and -not $InstallSmoke -and -not $PublishDryRun -and -not $Full) {
    exit 0
  }
  exit 1
}

try {
  if ($Build) {
    Invoke-Phase5Command 'npm' @('run', 'release:windows:slint')
    $artifacts = Get-SlintArtifactPaths
  }

  Invoke-Phase5Command 'node' @('.\scripts\verify-release-slint.mjs')

  if ($PublishDryRun) {
    Invoke-Phase5Command 'node' @('.\scripts\publish-updater-alpha-slint.mjs', '--dry-run')
  }

  if ($InstallSmoke) {
    if ($generatedTempInstallRoot -and (Test-Path $InstallRoot)) {
      Remove-Item -LiteralPath $InstallRoot -Recurse -Force
    }
    Invoke-Phase5Command 'pwsh' @(
      '-ExecutionPolicy',
      'Bypass',
      '-File',
      '.\scripts\smoke-release-slint.ps1',
      '-InstallRoot',
      $InstallRoot
    )
  }

  if ($InstallSmoke) {
    $report = New-ReportBase -CheckedCommands @($checkedCommands) -Artifacts $artifacts
    $report.status = 'passed'
    $report.installRoot = $InstallRoot
    $report.registryProbes = Get-RegistryProbes
    $report.uninstallCleanup = @{
      registryEntriesRemoved = $true
    }
    Write-Phase5SmokeReport -Report $report
    Write-Host 'Phase 5 Slint smoke completed successfully.'
  } else {
    $report = New-ReportBase -CheckedCommands @($checkedCommands) -Artifacts $artifacts
    $report.status = 'blocked'
    $report.prerequisiteGaps = @('Installer smoke was not requested; run with -InstallSmoke or -Full to produce a completed smoke report.')
    Write-Phase5SmokeReport -Report $report
    Write-Host 'Phase 5 Slint smoke checks completed; installer smoke was not requested.'
  }
} catch {
  $report = New-ReportBase -CheckedCommands @($checkedCommands) -Artifacts $artifacts
  $report.status = 'failed'
  $report.failedCommand = if ($script:lastCommand) { $script:lastCommand } else { 'Phase 5 Slint smoke orchestration' }
  $report.exitCode = 1
  $report.message = $_.Exception.Message
  Write-Phase5SmokeReport -Report $report
  throw
}
