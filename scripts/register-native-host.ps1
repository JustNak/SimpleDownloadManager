param(
  [Parameter(Mandatory = $true)]
  [string]$HostBinaryPath,

  [string]$ChromiumExtensionId = 'pkaojpfpjieklhinoibjibmjldohlmbb',

  [string]$EdgeExtensionId = 'pkaojpfpjieklhinoibjibmjldohlmbb',

  [string]$FirefoxExtensionId = 'simple-download-manager@example.com',

  [string]$InstallRoot = $(Split-Path -Parent $HostBinaryPath)
)

$ErrorActionPreference = 'Stop'

function Write-Manifest {
  param(
    [string]$TemplatePath,
    [string]$OutputPath,
    [hashtable]$Replacements
  )

  $content = Get-Content -Raw -Path $TemplatePath
  foreach ($key in $Replacements.Keys) {
    $content = $content.Replace($key, $Replacements[$key])
  }
  Set-Content -Path $OutputPath -Value $content -Encoding UTF8
}

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$bundledTemplatePath = Join-Path $PSScriptRoot 'chromium.template.json'
$templateRoot = if (Test-Path $bundledTemplatePath) {
  $PSScriptRoot
} else {
  Join-Path $workspaceRoot 'apps\native-host\manifests'
}
$manifestRoot = Join-Path $InstallRoot 'native-messaging'

New-Item -ItemType Directory -Force -Path $manifestRoot | Out-Null

$escapedHostPath = ($HostBinaryPath -replace '\\', '\\')

$chromiumManifestPath = Join-Path $manifestRoot 'com.myapp.download_manager.chrome.json'
$edgeManifestPath = Join-Path $manifestRoot 'com.myapp.download_manager.edge.json'
$firefoxManifestPath = Join-Path $manifestRoot 'com.myapp.download_manager.firefox.json'

Write-Manifest -TemplatePath (Join-Path $templateRoot 'chromium.template.json') -OutputPath $chromiumManifestPath -Replacements @{
  '__HOST_PATH__' = $escapedHostPath
  '__CHROMIUM_EXTENSION_ID__' = $ChromiumExtensionId
}

Write-Manifest -TemplatePath (Join-Path $templateRoot 'edge.template.json') -OutputPath $edgeManifestPath -Replacements @{
  '__HOST_PATH__' = $escapedHostPath
  '__EDGE_EXTENSION_ID__' = $EdgeExtensionId
}

Write-Manifest -TemplatePath (Join-Path $templateRoot 'firefox.template.json') -OutputPath $firefoxManifestPath -Replacements @{
  '__HOST_PATH__' = $escapedHostPath
  '__FIREFOX_EXTENSION_ID__' = $FirefoxExtensionId
}

function Set-RegistryDefaultValue {
  param(
    [string]$SubKey,
    [string]$Value
  )

  $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($SubKey)
  if ($null -eq $key) {
    throw "Could not create registry key HKCU:\$SubKey"
  }

  try {
    $key.SetValue('', $Value, [Microsoft.Win32.RegistryValueKind]::String)
  } finally {
    $key.Dispose()
  }
}

Set-RegistryDefaultValue -SubKey 'Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager' -Value $chromiumManifestPath
Set-RegistryDefaultValue -SubKey 'Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager' -Value $edgeManifestPath
Set-RegistryDefaultValue -SubKey 'Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager' -Value $firefoxManifestPath

Write-Host "Registered native host manifests:"
Write-Host "  Chrome : $chromiumManifestPath"
Write-Host "  Edge   : $edgeManifestPath"
Write-Host "  Firefox: $firefoxManifestPath"
