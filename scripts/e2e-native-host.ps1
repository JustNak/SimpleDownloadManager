param(
  [Parameter(Mandatory = $true)]
  [string]$DesktopBinaryPath,

  [Parameter(Mandatory = $true)]
  [string]$HostBinaryPath,

  [Parameter(Mandatory = $true)]
  [string]$DataDir,

  [string]$Url = 'https://example.com/'
)

$ErrorActionPreference = 'Stop'

function Invoke-NativeHost {
  param(
    [string]$HostPath,
    [string]$DesktopPath,
    [string]$AppDataDir,
    [hashtable]$Payload
  )

  $startInfo = New-Object System.Diagnostics.ProcessStartInfo
  $startInfo.FileName = $HostPath
  $startInfo.UseShellExecute = $false
  $startInfo.RedirectStandardInput = $true
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  $startInfo.Environment['MYAPP_DESKTOP_PATH'] = $DesktopPath
  $startInfo.Environment['MYAPP_DATA_DIR'] = $AppDataDir

  $process = [System.Diagnostics.Process]::Start($startInfo)
  try {
    $json = [System.Text.Encoding]::UTF8.GetBytes(($Payload | ConvertTo-Json -Compress -Depth 8))
    $length = [System.BitConverter]::GetBytes([uint32]$json.Length)

    $stdin = $process.StandardInput.BaseStream
    $stdin.Write($length, 0, $length.Length)
    $stdin.Write($json, 0, $json.Length)
    $stdin.Flush()
    $process.StandardInput.Close()

    $stdout = $process.StandardOutput.BaseStream
    $responseLengthBytes = New-Object byte[] 4
    $read = $stdout.Read($responseLengthBytes, 0, 4)
    if ($read -ne 4) {
      $stderr = $process.StandardError.ReadToEnd()
      throw "Native host did not return a framed response. STDERR: $stderr"
    }

    $responseLength = [System.BitConverter]::ToUInt32($responseLengthBytes, 0)
    $responseBytes = New-Object byte[] $responseLength
    $offset = 0
    while ($offset -lt $responseLength) {
      $chunkRead = $stdout.Read($responseBytes, $offset, $responseLength - $offset)
      if ($chunkRead -le 0) {
        break
      }

      $offset += $chunkRead
    }

    if ($offset -ne $responseLength) {
      throw "Incomplete native host response. Expected $responseLength bytes, received $offset."
    }

    return ([System.Text.Encoding]::UTF8.GetString($responseBytes) | ConvertFrom-Json)
  } finally {
    if (-not $process.HasExited) {
      $process.WaitForExit(10000) | Out-Null
      if (-not $process.HasExited) {
        $process.Kill($true)
      }
    }

    $process.Dispose()
  }
}

if (Test-Path $DataDir) {
  Remove-Item -Path $DataDir -Recurse -Force
}

New-Item -ItemType Directory -Path $DataDir | Out-Null

$desktopStart = New-Object System.Diagnostics.ProcessStartInfo
$desktopStart.FileName = $DesktopBinaryPath
$desktopStart.WorkingDirectory = (Split-Path -Parent $DesktopBinaryPath)
$desktopStart.UseShellExecute = $false
$desktopStart.Environment['MYAPP_DATA_DIR'] = $DataDir

$desktopProcess = [System.Diagnostics.Process]::Start($desktopStart)

try {
  Start-Sleep -Seconds 4

  $pingRequest = @{
    protocolVersion = 1
    requestId = [guid]::NewGuid().ToString()
    type = 'ping'
    payload = @{}
  }

  $enqueueRequest = @{
    protocolVersion = 1
    requestId = [guid]::NewGuid().ToString()
    type = 'enqueue_download'
    payload = @{
      url = $Url
      source = @{
        entryPoint = 'popup'
        browser = 'chrome'
        extensionVersion = '0.2.3-a'
      }
    }
  }

  $pingResponse = Invoke-NativeHost -HostPath $HostBinaryPath -DesktopPath $DesktopBinaryPath -AppDataDir $DataDir -Payload $pingRequest
  $enqueueResponse = Invoke-NativeHost -HostPath $HostBinaryPath -DesktopPath $DesktopBinaryPath -AppDataDir $DataDir -Payload $enqueueRequest

  $statePath = Join-Path $DataDir 'state.json'
  $job = $null
  for ($attempt = 0; $attempt -lt 60; $attempt++) {
    if (Test-Path $statePath) {
      $state = Get-Content -Raw -Path $statePath | ConvertFrom-Json
      if ($state.jobs.Count -gt 0) {
        $job = $state.jobs[0]
        if ($job.state -in @('completed', 'failed')) {
          break
        }
      }
    }

    Start-Sleep -Seconds 1
  }

  [pscustomobject]@{
    pingOk = $pingResponse.ok
    enqueueOk = $enqueueResponse.ok
    enqueueType = $enqueueResponse.type
    jobId = if ($enqueueResponse.payload) { $enqueueResponse.payload.jobId } else { $null }
    finalState = if ($job) { $job.state } else { $null }
    downloadedBytes = if ($job) { $job.downloadedBytes } else { 0 }
    totalBytes = if ($job) { $job.totalBytes } else { 0 }
    targetPath = if ($job) { $job.targetPath } else { $null }
    fileExists = if ($job -and $job.targetPath) { Test-Path $job.targetPath } else { $false }
    error = if ($job) { $job.error } else { $null }
    statePath = $statePath
  } | ConvertTo-Json -Depth 8
}
finally {
  if ($desktopProcess -and -not $desktopProcess.HasExited) {
    $desktopProcess.Kill($true)
  }

  if ($desktopProcess) {
    $desktopProcess.Dispose()
  }
}
