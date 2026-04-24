$ErrorActionPreference = 'Stop'

$paths = @(
  'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager',
  'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager',
  'HKCU:\Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager'
)

foreach ($path in $paths) {
  if (Test-Path $path) {
    Remove-Item -Path $path -Recurse -Force
    Write-Host "Removed $path"
  }
}
