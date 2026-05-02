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

function Resolve-MakensisPath {
  $command = Get-Command -Name 'makensis' -ErrorAction SilentlyContinue
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
  $probeRoot = Join-Path $env:TEMP "SimpleDownloadManager-SlintSigningProbe-$PID"
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

function Test-ReleasePrerequisites {
  $missing = @()

  if ($null -eq (Get-Command -Name 'cargo-packager' -ErrorAction SilentlyContinue)) {
    $missing += 'cargo-packager (install with: cargo install cargo-packager --locked)'
  }

  $makensisPath = Resolve-MakensisPath
  if ($null -eq $makensisPath) {
    $missing += 'NSIS makensis (install NSIS and make makensis available on PATH)'
  } else {
    Add-ExecutableDirectoryToPath $makensisPath
  }

  Import-LegacyTauriSigningEnvironment

  if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    if (-not [string]::IsNullOrWhiteSpace($env:CARGO_PACKAGER_SIGN_PRIVATE_KEY)) {
      $env:TAURI_SIGNING_PRIVATE_KEY = $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY
    } else {
      $missing += 'CARGO_PACKAGER_SIGN_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY'
    }
  }

  if (
    $null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -and
    $null -ne $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD
  ) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD
  }

  if ($missing.Count -eq 0) {
    $signingConfigurationError = Test-TauriSigningConfiguration
    if ($null -ne $signingConfigurationError) {
      $missing += $signingConfigurationError
    }
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

$packageJson = Get-Content (Join-Path $workspaceRoot 'package.json') -Raw | ConvertFrom-Json
$slintInstallerPath = Join-Path $slintReleaseRoot "bundle\nsis\simple-download-manager_$($packageJson.version)_x64-setup.exe"

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
    Remove-Item Env:CARGO_PACKAGER_SIGN_PRIVATE_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
    Invoke-ReleaseCommand 'cargo' @('packager', '--release')
  } finally {
    Pop-Location
  }

  Invoke-ReleaseCommand 'node' @('node_modules/@tauri-apps/cli/tauri.js', 'signer', 'sign', $slintInstallerPath)
  Invoke-ReleaseCommand 'node' @('.\scripts\updater-release.mjs', '--slint')
  Invoke-ReleaseCommand 'node' @('.\scripts\verify-release-slint.mjs')

  Write-Host "Slint release artifacts written to $slintReleaseRoot"
} finally {
  Pop-Location
}
