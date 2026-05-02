param(
  [switch]$Build,
  [switch]$CheckOnly,
  [string]$InstallRoot
)

$ErrorActionPreference = 'Stop'

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$generatedTempInstallRoot = $false
if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
  $InstallRoot = Join-Path ([System.IO.Path]::GetTempPath()) "SimpleDownloadManager-SlintSmoke-$PID"
  $generatedTempInstallRoot = $true
}
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)

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

function Test-SlintSmokePrerequisites {
  param([switch]$RequireBuildTools)

  $missing = New-Object System.Collections.Generic.List[string]
  if ($RequireBuildTools) {
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
    }
  }

  if ($missing.Count -gt 0) {
    throw "Missing Slint installer smoke prerequisites:`n - $($missing -join "`n - ")"
  }
}

function Invoke-SmokeCommand {
  param(
    [string]$Command,
    [string[]]$Arguments,
    [string]$WorkingDirectory = $workspaceRoot
  )

  Write-Host "> $Command $($Arguments -join ' ')"
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
    throw "$Command exited with code $($process.ExitCode)"
  }
}

function Get-SlintReleaseArtifactPaths {
  $script = @"
import { slintRequiredArtifactPaths } from './scripts/verify-release-slint.mjs';
const paths = await slintRequiredArtifactPaths({ root: process.cwd() });
console.log(JSON.stringify(paths));
"@
  $json = node --input-type=module -e $script
  if ($LASTEXITCODE -ne 0) {
    throw 'Could not resolve Slint release artifact paths.'
  }
  return $json | ConvertFrom-Json
}

function Test-RegistryValue {
  param(
    [string]$Path,
    [string]$ExpectedValue
  )

  $item = Get-Item -Path $Path -ErrorAction Stop
  $actual = $item.GetValue('')
  if ($actual -ne $ExpectedValue) {
    throw "Registry default value mismatch for $Path. Expected '$ExpectedValue', got '$actual'."
  }
}

function Test-RegistryValueMissing {
  param([string]$Path)

  if (Test-Path $Path) {
    throw "Registry key should have been removed by uninstall: $Path"
  }
}

function Wait-RegistryValueMissing {
  param(
    [string]$Path,
    [int]$Attempts = 60
  )

  for ($index = 0; $index -lt $Attempts; $index += 1) {
    if (-not (Test-Path $Path)) {
      return
    }
    Start-Sleep -Milliseconds 250
  }

  throw "Registry key should have been removed by uninstall: $Path"
}

Test-SlintSmokePrerequisites -RequireBuildTools:($Build -or $CheckOnly)

if ($Build) {
  Invoke-SmokeCommand 'npm' @('run', 'release:windows:slint')
}

if ($CheckOnly) {
  Invoke-SmokeCommand 'node' @('.\scripts\verify-release-slint.mjs')
  Write-Host 'Slint smoke check mode: prerequisite and artifact checks passed.'
  Write-Host "Install root that would be used: $InstallRoot"
  exit 0
}

Invoke-SmokeCommand 'node' @('.\scripts\verify-release-slint.mjs')
$artifacts = Get-SlintReleaseArtifactPaths
$installerPath = [string]$artifacts.installerPath
if (-not (Test-Path $installerPath)) {
  throw "Slint installer artifact is missing: $installerPath"
}

if ($generatedTempInstallRoot -and (Test-Path $InstallRoot)) {
  Remove-Item -LiteralPath $InstallRoot -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null

Write-Host "Installing Slint NSIS package into isolated smoke root: $InstallRoot"
Invoke-SmokeCommand $installerPath @('/S', "/D=$InstallRoot")

Invoke-SmokeCommand 'node' @('.\scripts\smoke-release-slint.mjs', '--install-root', $InstallRoot)

$chromeManifest = Join-Path $InstallRoot 'native-messaging\com.myapp.download_manager.chrome.json'
$edgeManifest = Join-Path $InstallRoot 'native-messaging\com.myapp.download_manager.edge.json'
$firefoxManifest = Join-Path $InstallRoot 'native-messaging\com.myapp.download_manager.firefox.json'

Test-RegistryValue 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager' $chromeManifest
Test-RegistryValue 'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager' $edgeManifest
Test-RegistryValue 'HKCU:\Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager' $firefoxManifest

$uninstallerPath = Join-Path $InstallRoot 'uninstall.exe'
if (-not (Test-Path $uninstallerPath)) {
  throw "Slint uninstaller is missing: $uninstallerPath"
}

Write-Host 'Running Slint uninstaller smoke step.'
Invoke-SmokeCommand $uninstallerPath @('/S')

Wait-RegistryValueMissing 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager'
Wait-RegistryValueMissing 'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager'
Wait-RegistryValueMissing 'HKCU:\Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager'

Write-Host 'Slint installer smoke completed successfully.'
