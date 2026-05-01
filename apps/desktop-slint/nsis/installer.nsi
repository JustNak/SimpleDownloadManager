; Slint NSIS template for cargo-packager 0.11.x.
; This template preserves the default packaging flow and adds native-host hooks.
; Keep Tauri startup-option prompts out of this Slint packaging slice.

!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "{{product_name}}"
OutFile "{{out_file}}"
InstallDir "$LOCALAPPDATA\Programs\{{product_name}}"
RequestExecutionLevel user
Unicode true
ShowInstDetails show
ShowUninstDetails show

!define PRODUCT_NAME "{{product_name}}"
!define VERSION "{{version}}"
!define MAIN_BINARY_NAME "{{main_binary_name}}"
!define MAIN_BINARY_SOURCE "{{main_binary_path}}"
!define UNINSTALLER "uninstall.exe"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "-Application"
  SetOutPath "$INSTDIR"

  File "${MAIN_BINARY_SOURCE}"

  {{#each resources_dirs}}
  CreateDirectory "$INSTDIR\{{this}}"
  {{/each}}

  {{#each resources}}
  File /a "/oname={{this}}" "{{@key}}"
  {{/each}}

  {{#each binaries}}
  File /a "/oname={{this}}" "{{@key}}"
  {{/each}}

  WriteUninstaller "$INSTDIR\${UNINSTALLER}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAIN_BINARY_NAME}.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\${UNINSTALLER}$\""
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" "1"

  Call RegisterNativeHost
SectionEnd

Section "Uninstall"
  Call un.UnregisterNativeHost
  Delete "$INSTDIR\${MAIN_BINARY_NAME}.exe"
  {{#each resources}}
  Delete "$INSTDIR\{{this}}"
  {{/each}}
  {{#each binaries}}
  Delete "$INSTDIR\{{this}}"
  {{/each}}
  Delete "$INSTDIR\${UNINSTALLER}"
  {{#each resources_dirs}}
  RMDir /REBOOTOK "$INSTDIR\{{this}}"
  {{/each}}
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTKEY}"
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
