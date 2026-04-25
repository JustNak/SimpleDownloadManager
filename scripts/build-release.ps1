$ErrorActionPreference = 'Stop'

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$releaseRoot = Join-Path $workspaceRoot 'release'
$desktopRoot = Join-Path $workspaceRoot 'apps\desktop'
$desktopTauriRoot = Join-Path $desktopRoot 'src-tauri'
$hostRoot = Join-Path $workspaceRoot 'apps\native-host'
$extensionRoot = Join-Path $workspaceRoot 'apps\extension'
$releaseTempRoot = Join-Path $workspaceRoot '.tmp\release'

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

if (Test-Path $releaseRoot) {
  Remove-Item -Path $releaseRoot -Recurse -Force
}

New-Item -ItemType Directory -Path $releaseRoot | Out-Null
New-Item -ItemType Directory -Path $releaseTempRoot -Force | Out-Null
$env:TMP = $releaseTempRoot
$env:TEMP = $releaseTempRoot

Push-Location $workspaceRoot
try {
  Invoke-ReleaseCommand -FilePath 'npm' -ArgumentList @('run', 'build:extension')
  Invoke-ReleaseCommand -FilePath 'npm' -ArgumentList @('run', 'build:desktop')

  Invoke-ReleaseCommand -FilePath 'cargo' -ArgumentList @('build', '--release', '--manifest-path', "$hostRoot\Cargo.toml")
  Invoke-ReleaseCommand -FilePath 'node' -ArgumentList @('.\scripts\prepare-release.mjs')

  $bundleDir = Join-Path $desktopTauriRoot 'target\release\bundle'
  if (Test-Path $bundleDir) {
    Remove-Item -Path $bundleDir -Recurse -Force
  }

  Invoke-ReleaseCommand -FilePath 'npm' -ArgumentList @('run', 'tauri:build', '--workspace', '@myapp/desktop')

  $chromiumZip = Join-Path $releaseRoot 'simple-download-manager-chromium-extension.zip'
  $firefoxZip = Join-Path $releaseRoot 'simple-download-manager-firefox-extension.zip'

  Compress-Archive -Path "$extensionRoot\dist\chromium\*" -DestinationPath $chromiumZip
  Compress-Archive -Path "$extensionRoot\dist\firefox\*" -DestinationPath $firefoxZip

  if (Test-Path $bundleDir) {
    Copy-Item -Path $bundleDir -Destination (Join-Path $releaseRoot 'bundle') -Recurse
  } else {
    throw "Tauri bundle directory was not produced: $bundleDir"
  }

  Copy-Item -Path "$hostRoot\target\release\simple-download-manager-native-host.exe" -Destination $releaseRoot
  Copy-Item -Path "$workspaceRoot\config\release.json" -Destination $releaseRoot

  Write-Host "Release artifacts written to $releaseRoot"
}
finally {
  Pop-Location
}
