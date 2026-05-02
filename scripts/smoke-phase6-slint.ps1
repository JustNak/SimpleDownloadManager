param(
  [switch]$CheckOnly,
  [switch]$Build,
  [switch]$RuntimeSmoke,
  [switch]$Full,
  [switch]$StartupRegistrySmoke,
  [switch]$InteractiveTrayCheck,
  [switch]$TrayConfirmed,
  [switch]$RequireCompletionEvidence,
  [string]$InstallRoot,
  [string]$DataDir
)

$ErrorActionPreference = 'Stop'

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$generatedInstallRoot = $false
$generatedDataDir = $false
$lastCommand = $null
$checkedCommands = New-Object System.Collections.Generic.List[string]
$checkedCommands.Add('npm run smoke:phase6:slint')

if ($Full) {
  $Build = $true
  $RuntimeSmoke = $true
}

if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
  $InstallRoot = Join-Path ([System.IO.Path]::GetTempPath()) "SimpleDownloadManager-SlintPhase6-$PID\Install"
  $generatedInstallRoot = $true
}
if ([string]::IsNullOrWhiteSpace($DataDir)) {
  $DataDir = Join-Path ([System.IO.Path]::GetTempPath()) "SimpleDownloadManager-SlintPhase6-$PID\Data"
  $generatedDataDir = $true
}

$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$DataDir = [System.IO.Path]::GetFullPath($DataDir)

$singleInstanceRequestId = 'desktop-single-instance'
$singleInstanceWakeRequestType = 'show_window'
$autostartArg = '--autostart'
$startupRegistryValueName = 'Simple Download Manager'
$startupRegistryPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

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

function Invoke-Phase6Command {
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

function Invoke-Phase6CommandOutput {
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
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  foreach ($argument in $Arguments) {
    [void]$startInfo.ArgumentList.Add($argument)
  }
  $process = [System.Diagnostics.Process]::Start($startInfo)
  $stdout = $process.StandardOutput.ReadToEnd()
  $stderr = $process.StandardError.ReadToEnd()
  $process.WaitForExit()
  if ($process.ExitCode -ne 0) {
    throw "$script:lastCommand exited with code $($process.ExitCode): $stderr"
  }
  if (-not [string]::IsNullOrWhiteSpace($stderr)) {
    Write-Host $stderr
  }
  return $stdout
}

function Write-Phase6Report {
  param([hashtable]$Report)

  $json = $Report | ConvertTo-Json -Compress -Depth 16
  $reportPath = Invoke-Phase6CommandOutput 'node' @(
    '.\scripts\slint-phase6-smoke-report.mjs',
    '--report-json',
    $json
  )
  Write-Host $reportPath.Trim()
}

function Get-SlintReleaseArtifactPaths {
  $script = @"
import { slintRequiredArtifactPaths } from './scripts/verify-release-slint.mjs';
const paths = await slintRequiredArtifactPaths({ root: process.cwd() });
console.log(JSON.stringify(paths));
"@
  $json = Invoke-Phase6CommandOutput 'node' @('--input-type=module', '-e', $script)
  return $json | ConvertFrom-Json
}

function Test-PathGap {
  param(
    [System.Collections.Generic.List[string]]$Gaps,
    [string]$Label,
    [string]$Path
  )

  if (-not (Test-Path -LiteralPath $Path)) {
    $Gaps.Add("$Label is missing: $Path")
  }
}

function Get-Phase6ArtifactReport {
  param($Paths)

  return [ordered]@{
    installerPath = [string]$Paths.installerPath
    signaturePath = [string]$Paths.signaturePath
    transitionMetadataPath = [string]$Paths.transitionMetadataPath
    nativeMetadataPath = [string]$Paths.metadataPath
  }
}

function Get-Phase6PrerequisiteGaps {
  param(
    [switch]$RequireArtifacts,
    [switch]$RequireBuildTools
  )

  $gaps = New-Object System.Collections.Generic.List[string]
  if ($RequireBuildTools) {
    if (-not (Get-Command 'cargo-packager' -ErrorAction SilentlyContinue)) {
      $gaps.Add('cargo-packager (install with: cargo install cargo-packager --locked)')
    }
    if (-not (Get-Command 'makensis' -ErrorAction SilentlyContinue)) {
      $candidateRoots = @(${env:ProgramFiles(x86)}, $env:ProgramFiles) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
      $makensis = $null
      foreach ($root in $candidateRoots) {
        foreach ($relativePath in @('NSIS\Bin\makensis.exe', 'NSIS\makensis.exe')) {
          $candidate = Join-Path $root $relativePath
          if (Test-Path -LiteralPath $candidate) {
            $makensis = $candidate
            break
          }
        }
        if ($null -ne $makensis) {
          break
        }
      }
      if ($null -eq $makensis) {
        $gaps.Add('makensis (install NSIS and ensure makensis.exe is on PATH)')
      }
    }
  }

  if ($RequireArtifacts) {
    try {
      $paths = Get-SlintReleaseArtifactPaths
      Test-PathGap $gaps 'Slint installer artifact' ([string]$paths.installerPath)
      Test-PathGap $gaps 'Slint installer signature' ([string]$paths.signaturePath)
      Test-PathGap $gaps 'Slint transition updater feed' ([string]$paths.transitionMetadataPath)
      Test-PathGap $gaps 'Slint-native updater feed' ([string]$paths.metadataPath)
    } catch {
      $gaps.Add("Could not resolve Slint release artifacts: $($_.Exception.Message)")
    }
  }

  return $gaps
}

function New-BaseReport {
  param([string]$Status)

  return [ordered]@{
    status = $Status
    timestamp = (Get-Date).ToUniversalTime().ToString('o')
    checkedCommands = @($checkedCommands)
    artifacts = [ordered]@{}
    installRoot = $null
    dataDir = $null
    nativeHostHandoff = $null
    singleInstance = $null
    startupRegistration = $null
    stateMigration = $null
    tray = $null
    cleanup = $null
    prerequisiteGaps = @()
    failedCommand = $null
    exitCode = $null
    message = $null
  }
}

function Start-SlintProcess {
  param(
    [string]$AppExe,
    [string]$AppDataDir
  )

  $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $AppExe
  $startInfo.WorkingDirectory = Split-Path -Parent $AppExe
  $startInfo.UseShellExecute = $false
  $startInfo.Environment['MYAPP_DATA_DIR'] = $AppDataDir
  return [System.Diagnostics.Process]::Start($startInfo)
}

function Invoke-SlintSmokeCommand {
  param(
    [string]$AppExe,
    [string]$Argument
  )

  $script:lastCommand = "$AppExe $Argument"
  Write-Host "> $script:lastCommand"
  $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $AppExe
  $startInfo.WorkingDirectory = Split-Path -Parent $AppExe
  $startInfo.UseShellExecute = $false
  $startInfo.Environment['MYAPP_ENABLE_SMOKE_COMMANDS'] = '1'
  [void]$startInfo.ArgumentList.Add($Argument)
  $process = [System.Diagnostics.Process]::Start($startInfo)
  $exited = $process.WaitForExit(15000)
  if (-not $exited) {
    Stop-SmokeProcess $process
    throw "$script:lastCommand did not exit within 15 seconds"
  }
  $exitCode = $process.ExitCode
  $process.Dispose()
  if ($exitCode -ne 0) {
    throw "$script:lastCommand exited with code $exitCode"
  }
}

function Stop-SmokeProcess {
  param($Process)

  if ($null -eq $Process) {
    return
  }

  try {
    if (-not $Process.HasExited) {
      $Process.Kill($true)
      $Process.WaitForExit(10000) | Out-Null
    }
  } catch [System.InvalidOperationException] {
    Write-Host 'Process already exited or disposed during cleanup.'
  } finally {
    try {
      $Process.Dispose()
    } catch [System.InvalidOperationException] {
      Write-Host 'Process already exited or disposed during cleanup.'
    }
  }
}

function Write-LegacyState {
  param([string]$AppDataDir)

  if (Test-Path -LiteralPath $AppDataDir) {
    Remove-Item -LiteralPath $AppDataDir -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $AppDataDir | Out-Null
  $downloadDir = Join-Path $AppDataDir 'Downloads'
  New-Item -ItemType Directory -Force -Path $downloadDir | Out-Null
  $statePath = Join-Path $AppDataDir 'state.json'
  $legacyState = [ordered]@{
    jobs = @(
      [ordered]@{
        id = 'legacy_job_1'
        url = 'https://example.com/legacy.bin'
        filename = 'legacy.bin'
        state = 'completed'
        progress = 100
        totalBytes = 1
        downloadedBytes = 1
        speed = 0
        eta = 0
        targetPath = (Join-Path $downloadDir 'legacy.bin')
        tempPath = (Join-Path $downloadDir 'legacy.bin.part')
      }
    )
    settings = [ordered]@{
      downloadDirectory = $downloadDir
      maxConcurrentDownloads = 3
      notificationsEnabled = $true
      theme = 'system'
    }
  }
  $legacyState | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $statePath -Encoding utf8
  return $statePath
}

function Test-StateMigration {
  param(
    [string]$StatePath,
    $Process
  )

  $state = $null
  for ($attempt = 0; $attempt -lt 30; $attempt += 1) {
    if (Test-Path -LiteralPath $StatePath) {
      $state = Get-Content -Raw -LiteralPath $StatePath | ConvertFrom-Json
      if ($state.settings.extensionIntegration -and $state.settings.torrent -and $state.jobs.Count -gt 0) {
        break
      }
    }
    if ($Process.HasExited) {
      break
    }
    Start-Sleep -Milliseconds 500
  }

  if ($null -eq $state) {
    throw "Slint did not rewrite migrated state: $StatePath"
  }

  return [ordered]@{
    stablePath = ([System.IO.Path]::GetFileName($StatePath) -eq 'state.json')
    loadedLegacyState = ($state.jobs.Count -gt 0 -and $state.jobs[0].id -eq 'legacy_job_1')
    rewroteCurrentState = ($null -ne $state.settings.extensionIntegration -and $null -ne $state.settings.torrent)
    jobCount = [int]$state.jobs.Count
    statePath = $StatePath
  }
}

function Test-SingleInstanceWake {
  param(
    [string]$AppExe,
    [string]$AppDataDir,
    $OriginalProcess
  )

  Start-Sleep -Seconds 2
  $duplicate = Start-SlintProcess $AppExe $AppDataDir
  $duplicateExited = $duplicate.WaitForExit(8000)
  if (-not $duplicateExited) {
    Stop-SmokeProcess $duplicate
  } else {
    $duplicate.Dispose()
  }

  return [ordered]@{
    duplicateExited = [bool]$duplicateExited
    originalAlive = [bool](-not $OriginalProcess.HasExited)
    requestId = $singleInstanceRequestId
    wakeRequest = $singleInstanceWakeRequestType
  }
}

function Get-StartupRegistrationEvidence {
  param([string]$AppExe)

  $command = '"' + $AppExe + '" ' + $autostartArg
  return [ordered]@{
    valueName = $startupRegistryValueName
    command = $command
    matchesInstalledExe = [bool]($command.Contains($AppExe) -and (Test-Path -LiteralPath $AppExe))
    hasAutostartArg = [bool]$command.Contains($autostartArg)
    registryMutated = $false
  }
}

function Get-StartupRegistryValue {
  $properties = Get-ItemProperty -Path $startupRegistryPath -Name $startupRegistryValueName -ErrorAction SilentlyContinue
  if ($null -eq $properties) {
    return $null
  }

  $property = $properties.PSObject.Properties[$startupRegistryValueName]
  if ($null -eq $property) {
    return $null
  }

  return [string]$property.Value
}

function Invoke-StartupRegistrySmoke {
  param([string]$AppExe)

  $enabledCommand = $null
  try {
    Invoke-SlintSmokeCommand $AppExe '--smoke-sync-autostart=enable'
    $enabledCommand = Get-StartupRegistryValue
    if ([string]::IsNullOrWhiteSpace($enabledCommand)) {
      throw 'Slint startup smoke command did not create the startup registry value.'
    }
    if (-not $enabledCommand.Contains($AppExe) -or -not $enabledCommand.Contains($autostartArg)) {
      throw "Startup registry value does not reference the installed Slint app and autostart flag: $enabledCommand"
    }
  } finally {
    Invoke-SlintSmokeCommand $AppExe '--smoke-sync-autostart=disable'
  }

  $removedCommand = Get-StartupRegistryValue
  return [ordered]@{
    valueName = $startupRegistryValueName
    command = $enabledCommand
    matchesInstalledExe = [bool]($enabledCommand.Contains($AppExe) -and (Test-Path -LiteralPath $AppExe))
    hasAutostartArg = [bool]$enabledCommand.Contains($autostartArg)
    registryMutated = $true
    removedAfterSmoke = [bool][string]::IsNullOrWhiteSpace($removedCommand)
  }
}

function Get-TrayInteractionEvidence {
  param($Process)

  if (-not $InteractiveTrayCheck -and -not $TrayConfirmed) {
    return [ordered]@{
      status = 'manual_required'
      note = 'Manual tray open/exit confirmation remains required for the final Phase 6 gate.'
    }
  }

  if ($InteractiveTrayCheck) {
    Write-Host ''
    Write-Host 'Manual Phase 6 tray smoke check:'
    Write-Host '  1. Close the Slint main window and confirm the app remains in the system tray.'
    Write-Host '  2. Open the app from the tray icon/menu.'
    Write-Host '  3. Use the tray Exit menu item and confirm the app process exits.'
    Write-Host ''
  }

  if ($TrayConfirmed -and -not $InteractiveTrayCheck) {
    return [ordered]@{
      status = 'passed'
      note = 'Operator pre-confirmed close-to-tray, tray Open, and tray Exit behavior.'
      operatorConfirmed = $true
    }
  }

  $confirmed = [bool]$TrayConfirmed
  if (-not $confirmed) {
    $answer = Read-Host 'Type CONFIRMED after completing the tray smoke steps, or press Enter to keep this gate manual-required'
    $confirmed = $answer -eq 'CONFIRMED'
  }

  if (-not $confirmed) {
    return [ordered]@{
      status = 'manual_required'
      note = 'Operator did not confirm tray open/exit behavior during this smoke run.'
    }
  }

  $exited = $Process.WaitForExit(30000)
  if (-not $exited) {
    return [ordered]@{
      status = 'manual_required'
      note = 'Operator confirmed tray steps, but the Slint app process did not exit after tray Exit.'
    }
  }

  return [ordered]@{
    status = 'passed'
    note = 'Operator confirmed close-to-tray, tray Open, and tray Exit behavior.'
    appExitedViaTray = $true
  }
}

function Get-Phase6CompletionGaps {
  param([hashtable]$Report)

  $gaps = New-Object System.Collections.Generic.List[string]
  if ($Report.status -ne 'passed') {
    $gaps.Add('Phase 6 runtime smoke did not produce a passed report.')
  }
  if (-not $Report.startupRegistration.registryMutated) {
    $gaps.Add('Startup registry mutation smoke was not recorded.')
  }
  if (-not $Report.startupRegistration.removedAfterSmoke) {
    $gaps.Add('Startup registry cleanup after smoke was not recorded.')
  }
  if ($Report.tray.status -ne 'passed') {
    $gaps.Add('Tray open/exit behavior was not confirmed.')
  }
  if (-not $Report.nativeHostHandoff.pingOk -or -not $Report.nativeHostHandoff.enqueueOk) {
    $gaps.Add('Native-host ping/enqueue handoff evidence was not recorded.')
  }
  if (-not $Report.singleInstance.duplicateExited -or -not $Report.singleInstance.originalAlive) {
    $gaps.Add('Single-instance wake evidence was not recorded.')
  }
  if (-not $Report.stateMigration.stablePath -or -not $Report.stateMigration.loadedLegacyState -or -not $Report.stateMigration.rewroteCurrentState) {
    $gaps.Add('Legacy state migration evidence was not recorded.')
  }
  if (-not $Report.cleanup.appExited -or -not $Report.cleanup.registryEntriesRemoved) {
    $gaps.Add('Runtime cleanup evidence was not recorded.')
  }

  return $gaps
}

function Test-Phase6CompletionEligible {
  param([hashtable]$Report)

  return [bool](
    ($Report.status -eq 'passed') `
      -and ([bool]$Report.nativeHostHandoff.pingOk) `
      -and ([bool]$Report.nativeHostHandoff.enqueueOk) `
      -and ([bool]$Report.singleInstance.duplicateExited) `
      -and ([bool]$Report.singleInstance.originalAlive) `
      -and ([bool]$Report.stateMigration.stablePath) `
      -and ([bool]$Report.stateMigration.loadedLegacyState) `
      -and ([bool]$Report.stateMigration.rewroteCurrentState) `
      -and ([bool]$Report.startupRegistration.registryMutated) `
      -and ([bool]$Report.startupRegistration.removedAfterSmoke) `
      -and ($Report.tray.status -eq 'passed') `
      -and ([bool]$Report.cleanup.appExited) `
      -and ([bool]$Report.cleanup.registryEntriesRemoved)
  )
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

function Invoke-RuntimeSmoke {
  param($Artifacts)

  $appExe = Join-Path $InstallRoot 'simple-download-manager.exe'
  $hostExe = Join-Path $InstallRoot 'simple-download-manager-native-host.exe'
  $uninstaller = Join-Path $InstallRoot 'uninstall.exe'
  $appProcess = $null
  $cleanup = [ordered]@{
    appExited = $false
    registryEntriesRemoved = $false
    installRootRemoved = $false
  }

  if ($generatedInstallRoot -and (Test-Path -LiteralPath $InstallRoot)) {
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null

  try {
    Invoke-Phase6Command ([string]$Artifacts.installerPath) @('/S', "/D=$InstallRoot")
    Invoke-Phase6Command 'node' @('.\scripts\smoke-release-slint.mjs', '--install-root', $InstallRoot)

    $statePath = Write-LegacyState $DataDir
    $appProcess = Start-SlintProcess $appExe $DataDir
    $stateMigration = Test-StateMigration $statePath $appProcess
    $singleInstance = Test-SingleInstanceWake $appExe $DataDir $appProcess
    $trayEvidence = Get-TrayInteractionEvidence $appProcess
    Stop-SmokeProcess $appProcess
    $appProcess = $null
    $cleanup.appExited = $true

    $handoffDataDir = Join-Path (Split-Path -Parent $DataDir) 'NativeHostData'
    $handoffJson = Invoke-Phase6CommandOutput 'pwsh' @(
      '-NoProfile',
      '-ExecutionPolicy',
      'Bypass',
      '-File',
      '.\scripts\e2e-native-host.ps1',
      '-DesktopBinaryPath',
      $appExe,
      '-HostBinaryPath',
      $hostExe,
      '-DataDir',
      $handoffDataDir,
      '-Url',
      'https://example.com/simple-download-manager-phase6.bin'
    )
    $handoff = $handoffJson | ConvertFrom-Json

    $startupRegistration = if ($StartupRegistrySmoke) {
      Invoke-StartupRegistrySmoke $appExe
    } else {
      Get-StartupRegistrationEvidence $appExe
    }

    if (Test-Path -LiteralPath $uninstaller) {
      Invoke-Phase6Command $uninstaller @('/S')
    }
    Wait-RegistryValueMissing 'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager'
    Wait-RegistryValueMissing 'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager'
    Wait-RegistryValueMissing 'HKCU:\Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager'
    $cleanup.registryEntriesRemoved = $true

    if ($generatedInstallRoot -and (Test-Path -LiteralPath $InstallRoot)) {
      Remove-Item -LiteralPath $InstallRoot -Recurse -Force
    }
    if ($generatedDataDir -and (Test-Path -LiteralPath (Split-Path -Parent $DataDir))) {
      Remove-Item -LiteralPath (Split-Path -Parent $DataDir) -Recurse -Force
    }
    $cleanup.installRootRemoved = [bool](-not (Test-Path -LiteralPath $InstallRoot))

    return [ordered]@{
      installRoot = $InstallRoot
      dataDir = $DataDir
      nativeHostHandoff = [ordered]@{
        pingOk = [bool]$handoff.pingOk
        enqueueOk = [bool]$handoff.enqueueOk
        enqueueType = [string]$handoff.enqueueType
        jobId = [string]$handoff.jobId
        statePath = [string]$handoff.statePath
      }
      singleInstance = $singleInstance
      startupRegistration = $startupRegistration
      stateMigration = $stateMigration
      tray = $trayEvidence
      cleanup = $cleanup
    }
  } finally {
    Stop-SmokeProcess $appProcess
  }
}

try {
  if ($Build) {
    $checkedCommands.Add('npm run release:windows:slint')
  }
  if ($RuntimeSmoke) {
    $checkedCommands.Add('pwsh -ExecutionPolicy Bypass -File ".\scripts\e2e-native-host.ps1"')
  }
  if ($StartupRegistrySmoke) {
    $checkedCommands.Add('MYAPP_ENABLE_SMOKE_COMMANDS=1 simple-download-manager.exe --smoke-sync-autostart=enable')
    $checkedCommands.Add('MYAPP_ENABLE_SMOKE_COMMANDS=1 simple-download-manager.exe --smoke-sync-autostart=disable')
  }
  if ($InteractiveTrayCheck -or $TrayConfirmed) {
    $checkedCommands.Add('manual Slint tray close/open/exit confirmation')
  }

  $gaps = Get-Phase6PrerequisiteGaps -RequireArtifacts:(-not $Build) -RequireBuildTools:$Build
  if ($gaps.Count -gt 0) {
    $report = New-BaseReport 'blocked'
    $report.prerequisiteGaps = @($gaps)
    Write-Phase6Report $report
    Write-Host "Phase 6 Slint smoke blocked:`n - $($gaps -join "`n - ")"
    exit 0
  }

  if ($Build) {
    Invoke-Phase6Command 'npm' @('run', 'release:windows:slint')
  }

  Invoke-Phase6Command 'node' @('.\scripts\verify-release-slint.mjs')
  $artifactPaths = Get-SlintReleaseArtifactPaths
  $artifacts = Get-Phase6ArtifactReport $artifactPaths

  if (-not $RuntimeSmoke) {
    $report = New-BaseReport 'blocked'
    $report.artifacts = $artifacts
    $report.prerequisiteGaps = @('Runtime smoke was not requested; run with -RuntimeSmoke or -Full to produce Phase 6 runtime evidence.')
    Write-Phase6Report $report
    Write-Host 'Phase 6 Slint smoke checks completed; runtime smoke was not requested.'
    exit 0
  }

  $runtimeEvidence = Invoke-RuntimeSmoke $artifacts
  $report = New-BaseReport 'passed'
  $report.artifacts = $artifacts
  $report.installRoot = $runtimeEvidence.installRoot
  $report.dataDir = $runtimeEvidence.dataDir
  $report.nativeHostHandoff = $runtimeEvidence.nativeHostHandoff
  $report.singleInstance = $runtimeEvidence.singleInstance
  $report.startupRegistration = $runtimeEvidence.startupRegistration
  $report.stateMigration = $runtimeEvidence.stateMigration
  $report.tray = $runtimeEvidence.tray
  $report.cleanup = $runtimeEvidence.cleanup
  if ($RequireCompletionEvidence -and -not (Test-Phase6CompletionEligible $report)) {
    $blockedReport = New-BaseReport 'blocked'
    $blockedReport.artifacts = $report.artifacts
    $blockedReport.installRoot = $report.installRoot
    $blockedReport.dataDir = $report.dataDir
    $blockedReport.nativeHostHandoff = $report.nativeHostHandoff
    $blockedReport.singleInstance = $report.singleInstance
    $blockedReport.startupRegistration = $report.startupRegistration
    $blockedReport.stateMigration = $report.stateMigration
    $blockedReport.tray = $report.tray
    $blockedReport.cleanup = $report.cleanup
    $completionGaps = @(Get-Phase6CompletionGaps $report)
    if ($completionGaps.Count -eq 0) {
      $completionGaps = @('Phase 6 completion evidence is incomplete.')
    }
    $blockedReport.prerequisiteGaps = @($completionGaps)
    Write-Phase6Report $blockedReport
    Write-Host "Phase 6 Slint completion evidence blocked:`n - $($completionGaps -join "`n - ")"
    exit 0
  }
  Write-Phase6Report $report
  Write-Host 'Phase 6 Slint runtime smoke completed successfully.'
} catch {
  $report = New-BaseReport 'failed'
  $report.failedCommand = if ($script:lastCommand) { $script:lastCommand } else { 'Phase 6 Slint smoke orchestration' }
  $report.exitCode = 1
  $report.message = $_.Exception.Message
  Write-Phase6Report $report
  throw
}
