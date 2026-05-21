param(
  [string[]] $Targets = @('x64', 'arm64')
)

$ErrorActionPreference = 'Stop'

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$releaseRoot = Join-Path $workspaceRoot 'release'
$desktopRoot = Join-Path $workspaceRoot 'apps\desktop'
$desktopTauriRoot = Join-Path $desktopRoot 'src-tauri'
$hostRoot = Join-Path $workspaceRoot 'apps\native-host'
$extensionRoot = Join-Path $workspaceRoot 'apps\extension'
$releaseTempRoot = Join-Path $workspaceRoot '.tmp\release'
$defaultSigningKeyPath = Join-Path $env:USERPROFILE '.simple-download-manager\tauri-updater.key'
$signingKeyPath = $env:SDM_TAURI_SIGNING_PRIVATE_KEY_PATH

if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
  if ([string]::IsNullOrWhiteSpace($signingKeyPath)) {
    $signingKeyPath = $defaultSigningKeyPath
  }

  if (Test-Path -LiteralPath $signingKeyPath) {
    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -LiteralPath $signingKeyPath -Raw
  } else {
    throw "TAURI_SIGNING_PRIVATE_KEY is required to build signed updater artifacts. Set TAURI_SIGNING_PRIVATE_KEY, set SDM_TAURI_SIGNING_PRIVATE_KEY_PATH, or place the key at $defaultSigningKeyPath."
  }
}

if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
  if (![string]::IsNullOrWhiteSpace($env:SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH) -and (Test-Path -LiteralPath $env:SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH)) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -LiteralPath $env:SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH -Raw).TrimEnd()
  } else {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
  }
}

function Invoke-ReleaseCommand {
  param(
    [Parameter(Mandatory = $true)]
    [string] $FilePath,

    [string[]] $ArgumentList = @()
  )

  & $FilePath @ArgumentList
  if ($LASTEXITCODE -ne 0) {
    throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($ArgumentList -join ' ')"
  }
}

function Resolve-WindowsReleaseTarget {
  param(
    [Parameter(Mandatory = $true)]
    [string] $Target
  )

  switch ($Target.ToLowerInvariant()) {
    { $_ -in @('x64', 'amd64', 'x86_64', 'x86_64-pc-windows-msvc') } {
      return [PSCustomObject]@{
        Name = 'x64'
        RustTarget = 'x86_64-pc-windows-msvc'
        VsArch = 'amd64'
      }
    }
    { $_ -in @('arm64', 'aarch64', 'aarch64-pc-windows-msvc') } {
      return [PSCustomObject]@{
        Name = 'arm64'
        RustTarget = 'aarch64-pc-windows-msvc'
        VsArch = 'arm64'
      }
    }
    default {
      throw "Unsupported Windows release target: $Target"
    }
  }
}

function Import-VsDevEnvironment {
  param(
    [Parameter(Mandatory = $true)]
    [string] $Architecture
  )

  $programFilesX86 = ${env:ProgramFiles(x86)}
  $vsDevCmd = Join-Path $programFilesX86 'Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat'
  if (!(Test-Path -LiteralPath $vsDevCmd)) {
    throw "Visual Studio Build Tools developer command file was not found: $vsDevCmd"
  }

  $environment = & cmd.exe /d /s /c "`"$vsDevCmd`" -arch=$Architecture -host_arch=amd64 >nul && set"
  if ($LASTEXITCODE -ne 0) {
    throw "Could not load Visual Studio developer environment for $Architecture."
  }

  foreach ($line in $environment) {
    $separatorIndex = $line.IndexOf('=')
    if ($separatorIndex -le 0) {
      continue
    }

    $name = $line.Substring(0, $separatorIndex)
    $value = $line.Substring($separatorIndex + 1)
    Set-Item -Path "Env:$name" -Value $value
  }
}

$releaseTargets = @(
  $Targets |
    ForEach-Object { $_ -split ',' } |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ } |
    ForEach-Object { Resolve-WindowsReleaseTarget -Target $_ }
)

if (Test-Path $releaseRoot) {
  Remove-Item -Path $releaseRoot -Recurse -Force
}

New-Item -ItemType Directory -Path $releaseRoot | Out-Null
New-Item -ItemType Directory -Path $releaseTempRoot -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $releaseRoot 'bundle\nsis') -Force | Out-Null
$env:TMP = $releaseTempRoot
$env:TEMP = $releaseTempRoot

Push-Location $workspaceRoot
try {
  Invoke-ReleaseCommand -FilePath 'npm' -ArgumentList @('run', 'build:extension')
  Invoke-ReleaseCommand -FilePath 'npm' -ArgumentList @('run', 'build:desktop')

  foreach ($target in $releaseTargets) {
    Import-VsDevEnvironment -Architecture $target.VsArch

    Invoke-ReleaseCommand -FilePath 'cargo' -ArgumentList @(
      'build',
      '--release',
      '--manifest-path',
      "$hostRoot\Cargo.toml",
      '--target',
      $target.RustTarget
    )

    $targetConfigPath = Join-Path $releaseTempRoot "tauri-$($target.Name).conf.json"
    Invoke-ReleaseCommand -FilePath 'node' -ArgumentList @(
      '.\scripts\prepare-release.mjs',
      '--target',
      $target.RustTarget,
      '--config-out',
      $targetConfigPath
    )

    $bundleDir = Join-Path $desktopTauriRoot "target\$($target.RustTarget)\release\bundle"
    if (Test-Path $bundleDir) {
      Remove-Item -Path $bundleDir -Recurse -Force
    }

    Invoke-ReleaseCommand -FilePath 'npm' -ArgumentList @(
      'run',
      'tauri:build',
      '--workspace',
      '@myapp/desktop',
      '--',
      '--target',
      $target.RustTarget,
      '--bundles',
      'nsis',
      '--config',
      $targetConfigPath,
      '--',
      '--bin',
      'simple-download-manager-desktop-backend'
    )

    $targetNsisDir = Join-Path $bundleDir 'nsis'
    if (Test-Path $targetNsisDir) {
      Copy-Item -Path "$targetNsisDir\*" -Destination (Join-Path $releaseRoot 'bundle\nsis') -Force
    } else {
      throw "Tauri NSIS bundle directory was not produced: $targetNsisDir"
    }

    $targetHostBinary = Join-Path $hostRoot "target\$($target.RustTarget)\release\simple-download-manager-native-host.exe"
    Copy-Item -Path $targetHostBinary -Destination (Join-Path $releaseRoot "simple-download-manager-native-host-$($target.RustTarget).exe")
    if ($target.Name -eq 'x64') {
      Copy-Item -Path $targetHostBinary -Destination (Join-Path $releaseRoot 'simple-download-manager-native-host.exe')
    }
  }

  $chromiumZip = Join-Path $releaseRoot 'simple-download-manager-chromium-extension.zip'
  $firefoxZip = Join-Path $releaseRoot 'simple-download-manager-firefox-extension.zip'

  Compress-Archive -Path "$extensionRoot\dist\chromium\*" -DestinationPath $chromiumZip
  Compress-Archive -Path "$extensionRoot\dist\firefox\*" -DestinationPath $firefoxZip

  Copy-Item -Path "$workspaceRoot\config\release.json" -Destination $releaseRoot
  Invoke-ReleaseCommand -FilePath 'node' -ArgumentList @('.\scripts\updater-release.mjs')

  Write-Host "Release artifacts written to $releaseRoot"
}
finally {
  Pop-Location
}
