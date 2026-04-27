!macro NSIS_HOOK_POSTINSTALL
  StrCpy $0 "$INSTDIR\simple-download-manager-native-host.exe"
  IfFileExists "$0" found_sidecar 0

  StrCpy $0 "$INSTDIR\simple-download-manager-native-host-x86_64-pc-windows-msvc.exe"
  IfFileExists "$0" found_sidecar 0
  Goto done_postinstall

  found_sidecar:
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\register-native-host.ps1" -HostBinaryPath "$0" -InstallRoot "$INSTDIR"'

  done_postinstall:
  StrCmp $UpdateMode 1 relaunch_after_update 0
  StrCmp $PassiveMode 1 done_startup_options 0
  IfSilent done_startup_options 0

  MessageBox MB_YESNO|MB_ICONQUESTION "Start Simple Download Manager when Windows starts?" IDYES startup_options_yes IDNO done_startup_options

  startup_options_yes:
  MessageBox MB_YESNO|MB_ICONQUESTION "Start minimized to tray when Windows starts? This enables Tray Only startup mode." IDYES startup_options_tray IDNO startup_options_open

  startup_options_open:
  nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --installer-configure --installer-startup'
  Goto done_startup_options

  startup_options_tray:
  nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --installer-configure --installer-startup --installer-tray'
  Goto done_startup_options

  relaunch_after_update:
  Exec '"$INSTDIR\${MAINBINARYNAME}.exe"'

  done_startup_options:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\unregister-native-host.ps1"'
!macroend
