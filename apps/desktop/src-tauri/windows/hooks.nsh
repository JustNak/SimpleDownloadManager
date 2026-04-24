!macro NSIS_HOOK_POSTINSTALL
  StrCpy $0 "$INSTDIR\simple-download-manager-native-host.exe"
  IfFileExists "$0" found_sidecar 0

  StrCpy $0 "$INSTDIR\simple-download-manager-native-host-x86_64-pc-windows-msvc.exe"
  IfFileExists "$0" found_sidecar 0
  Goto done_postinstall

  found_sidecar:
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\register-native-host.ps1" -HostBinaryPath "$0" -InstallRoot "$INSTDIR"'

  done_postinstall:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\unregister-native-host.ps1"'
!macroend
