Var StartupCheckbox
Var StartupTrayCheckbox
Var StartupCheckboxState
Var StartupTrayCheckboxState
Var StartupOptionsApplyState

Function PageStartupOptions
  ${If} $PassiveMode = 1
    Abort
  ${EndIf}
  ${If} $UpdateMode = 1
    Abort
  ${EndIf}
  IfSilent 0 +2
    Abort

  !insertmacro MUI_HEADER_TEXT "Startup options" "Choose how Simple Download Manager starts."
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${IfThen} $(^RTL) == 1 ${|} nsDialogs::SetRTL $(^RTL) ${|}

  ${NSD_CreateLabel} 0 0 100% 24u "Choose whether Simple Download Manager starts automatically after you sign in."
  Pop $0

  ${NSD_CreateCheckbox} 0 42u 100% 12u "Start Simple Download Manager with Windows"
  Pop $StartupCheckbox
  SendMessage $StartupCheckbox ${BM_SETCHECK} ${BST_CHECKED} 0
  ${NSD_OnClick} $StartupCheckbox UpdateStartupTrayCheckbox

  ${NSD_CreateCheckbox} 18u 64u 100% 12u "Start minimized to tray"
  Pop $StartupTrayCheckbox
  SendMessage $StartupTrayCheckbox ${BM_SETCHECK} ${BST_CHECKED} 0

  ${NSD_CreateLabel} 18u 82u 100% 24u "Keeps the app available from the notification area without opening the main window."
  Pop $0

  Call UpdateStartupTrayCheckbox
  nsDialogs::Show
FunctionEnd

Function UpdateStartupTrayCheckbox
  ${NSD_GetState} $StartupCheckbox $StartupCheckboxState
  ${If} $StartupCheckboxState == ${BST_CHECKED}
    EnableWindow $StartupTrayCheckbox 1
  ${Else}
    SendMessage $StartupTrayCheckbox ${BM_SETCHECK} ${BST_UNCHECKED} 0
    EnableWindow $StartupTrayCheckbox 0
  ${EndIf}
FunctionEnd

Function PageLeaveStartupOptions
  ${NSD_GetState} $StartupCheckbox $StartupCheckboxState
  ${NSD_GetState} $StartupTrayCheckbox $StartupTrayCheckboxState
  StrCpy $StartupOptionsApplyState 1
FunctionEnd

Function ApplyStartupOptions
  ${If} $StartupOptionsApplyState != 1
    Return
  ${EndIf}

  ${If} $StartupCheckboxState == ${BST_CHECKED}
    ${If} $StartupTrayCheckboxState == ${BST_CHECKED}
      nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --installer-configure --installer-startup --installer-tray'
    ${Else}
      nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --installer-configure --installer-startup'
    ${EndIf}
  ${Else}
    nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --installer-configure'
  ${EndIf}
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  StrCpy $0 "$INSTDIR\simple-download-manager-native-host.exe"
  IfFileExists "$0" found_sidecar 0

  StrCpy $0 "$INSTDIR\simple-download-manager-native-host-x86_64-pc-windows-msvc.exe"
  IfFileExists "$0" found_sidecar 0

  StrCpy $0 "$INSTDIR\simple-download-manager-native-host-aarch64-pc-windows-msvc.exe"
  IfFileExists "$0" found_sidecar 0
  Goto done_postinstall

  found_sidecar:
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\register-native-host.ps1" -HostBinaryPath "$0" -InstallRoot "$INSTDIR"'

  done_postinstall:
  Call ApplyStartupOptions
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\unregister-native-host.ps1"'
!macroend
