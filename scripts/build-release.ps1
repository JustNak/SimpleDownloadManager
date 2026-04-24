$ErrorActionPreference = 'Stop'

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$releaseRoot = Join-Path $workspaceRoot 'release'
$desktopRoot = Join-Path $workspaceRoot 'apps\desktop'
$desktopTauriRoot = Join-Path $desktopRoot 'src-tauri'
$hostRoot = Join-Path $workspaceRoot 'apps\native-host'
$extensionRoot = Join-Path $workspaceRoot 'apps\extension'

if (Test-Path $releaseRoot) {
  Remove-Item -Path $releaseRoot -Recurse -Force
}

New-Item -ItemType Directory -Path $releaseRoot | Out-Null

Push-Location $workspaceRoot
try {
  npm run build:extension
  npm run build:desktop

  cargo build --release --manifest-path "$hostRoot\Cargo.toml"
  node .\scripts\prepare-release.mjs
  npm run tauri:build --workspace @myapp/desktop

  $chromiumZip = Join-Path $releaseRoot 'simple-download-manager-chromium-extension.zip'
  $firefoxZip = Join-Path $releaseRoot 'simple-download-manager-firefox-extension.zip'

  Compress-Archive -Path "$extensionRoot\dist\chromium\*" -DestinationPath $chromiumZip
  Compress-Archive -Path "$extensionRoot\dist\firefox\*" -DestinationPath $firefoxZip

  $bundleDir = Join-Path $desktopTauriRoot 'target\release\bundle'
  if (Test-Path $bundleDir) {
    Copy-Item -Path $bundleDir -Destination (Join-Path $releaseRoot 'bundle') -Recurse
  }

  Copy-Item -Path "$hostRoot\target\release\simple-download-manager-native-host.exe" -Destination $releaseRoot
  Copy-Item -Path "$workspaceRoot\config\release.json" -Destination $releaseRoot

  Write-Host "Release artifacts written to $releaseRoot"
}
finally {
  Pop-Location
}
