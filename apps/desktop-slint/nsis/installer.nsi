; Slint NSIS template for cargo-packager 0.11.x.
; This template preserves the default packaging flow and adds native-host hooks.
; Keep Tauri startup-option prompts out of this Slint packaging slice.

!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "{{productName}}"
OutFile "{{outFile}}"
InstallDir "$LOCALAPPDATA\Programs\{{productName}}"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show

!define PRODUCT_NAME "{{productName}}"
!define MAIN_BINARY "{{mainBinaryName}}"
!define UNINSTALLER "uninstall.exe"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "-Application"
  SetOutPath "$INSTDIR"
  {{#each files}}
  File /r "{{this}}"
  {{/each}}

  WriteUninstaller "$INSTDIR\${UNINSTALLER}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "UninstallString" "$INSTDIR\${UNINSTALLER}"

  Call RegisterNativeHost
SectionEnd

Section "Uninstall"
  Call un.UnregisterNativeHost
  Delete "$INSTDIR\${UNINSTALLER}"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
SectionEnd

Function RegisterNativeHost
  StrCpy $0 "$INSTDIR\simple-download-manager-native-host.exe"
  IfFileExists "$0" found_sidecar 0
  Goto done_register_native_host

  found_sidecar:
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\register-native-host.ps1" -HostBinaryPath "$0" -InstallRoot "$INSTDIR"'

  done_register_native_host:
FunctionEnd

Function un.UnregisterNativeHost
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -ExecutionPolicy Bypass -File "$INSTDIR\resources\install\unregister-native-host.ps1"'
FunctionEnd
