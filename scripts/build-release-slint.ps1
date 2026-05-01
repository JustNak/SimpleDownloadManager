param(
  [switch]$SkipExtension
)

$ErrorActionPreference = 'Stop'

function Invoke-ReleaseCommand {
  param(
    [string]$FilePath,
    [string[]]$Arguments
  )

  Write-Host "Running: $FilePath $($Arguments -join ' ')"
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$FilePath failed with exit code $LASTEXITCODE"
  }
}

function Test-ReleasePrerequisites {
  $missing = @()

  if ($null -eq (Get-Command -Name 'cargo-packager' -ErrorAction SilentlyContinue)) {
    $missing += 'cargo-packager (install with: cargo install cargo-packager --locked)'
  }

  if ($null -eq (Get-Command -Name 'makensis' -ErrorAction SilentlyContinue)) {
    $missing += 'NSIS makensis (install NSIS and make makensis available on PATH)'
  }

  if ([string]::IsNullOrWhiteSpace($env:CARGO_PACKAGER_SIGN_PRIVATE_KEY)) {
    if (-not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
      $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY = $env:TAURI_SIGNING_PRIVATE_KEY
    } else {
      $missing += 'CARGO_PACKAGER_SIGN_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY'
    }
  }

  if (
    [string]::IsNullOrWhiteSpace($env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD) -and
    -not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)
  ) {
    $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  }

  if ($missing.Count -gt 0) {
    throw "Missing Slint release prerequisites:`n - $($missing -join "`n - ")"
  }
}

$workspaceRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$desktopSlintRoot = Join-Path $workspaceRoot 'apps\desktop-slint'
$hostRoot = Join-Path $workspaceRoot 'apps\native-host'
$extensionRoot = Join-Path $workspaceRoot 'apps\extension'
$releaseRoot = Join-Path $workspaceRoot 'release'
$slintReleaseRoot = Join-Path $releaseRoot 'slint'

Test-ReleasePrerequisites

if (-not (Test-Path $releaseRoot)) {
  New-Item -ItemType Directory -Path $releaseRoot | Out-Null
}

if (Test-Path $slintReleaseRoot) {
  Remove-Item -Recurse -Force $slintReleaseRoot
}

New-Item -ItemType Directory -Path $slintReleaseRoot | Out-Null

Push-Location $workspaceRoot
try {
  if (-not $SkipExtension) {
    Invoke-ReleaseCommand 'npm' @('run', 'build:extension')

    Compress-Archive `
      -Path (Join-Path $extensionRoot 'dist\chromium\*') `
      -DestinationPath (Join-Path $slintReleaseRoot 'simple-download-manager-chromium-extension.zip') `
      -Force

    Compress-Archive `
      -Path (Join-Path $extensionRoot 'dist\firefox\*') `
      -DestinationPath (Join-Path $slintReleaseRoot 'simple-download-manager-firefox-extension.zip') `
      -Force
  }

  Invoke-ReleaseCommand 'cargo' @('build', '--release', '--manifest-path', (Join-Path $hostRoot 'Cargo.toml'))
  Invoke-ReleaseCommand 'cargo' @('build', '--release', '--manifest-path', (Join-Path $desktopSlintRoot 'Cargo.toml'))
  Invoke-ReleaseCommand 'node' @('.\scripts\prepare-release-slint.mjs')

  Push-Location $desktopSlintRoot
  try {
    Invoke-ReleaseCommand 'cargo' @('packager', '--release')
  } finally {
    Pop-Location
  }

  Invoke-ReleaseCommand 'node' @('.\scripts\updater-release.mjs', '--slint')
  Invoke-ReleaseCommand 'node' @('.\scripts\verify-release-slint.mjs')

  Write-Host "Slint release artifacts written to $slintReleaseRoot"
} finally {
  Pop-Location
}
