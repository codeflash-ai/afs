!include LogicLib.nsh

!define AFS_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define AFS_RUN_VALUE "AFS"
!define AFS_SHIM_MARKER "rem AFS_TERMINAL_CLI_SHIM"

!macro STOP_AFS_PROCESS_IMAGE IMAGE_NAME
  DetailPrint "Stopping ${IMAGE_NAME} if running..."
  ClearErrors
  ExecWait '"$SYSDIR\taskkill.exe" /F /T /IM "${IMAGE_NAME}"' $0
  ClearErrors
!macroend

!macro STOP_AFS_SIDECAR_PROCESSES
  !insertmacro STOP_AFS_PROCESS_IMAGE "afs-cloud-files.exe"
  !insertmacro STOP_AFS_PROCESS_IMAGE "afsd.exe"
  !insertmacro STOP_AFS_PROCESS_IMAGE "afs.exe"
  Sleep 500
!macroend

!macro DELETE_AFS_TERMINAL_SHIM SHIM_PATH
  ClearErrors
  FileOpen $0 "${SHIM_PATH}" r
  ${IfNot} ${Errors}
    FileRead $0 $1
    FileRead $0 $2
    FileClose $0
    ${If} $2 == "${AFS_SHIM_MARKER}$\r$\n"
    ${OrIf} $2 == "${AFS_SHIM_MARKER}$\n"
      Delete "${SHIM_PATH}"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro STOP_AFS_SIDECAR_PROCESSES
!macroend

!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  File /oname=afs.exe "${__FILEDIR__}\..\..\..\..\apps\desktop\src-tauri\windows\afs.exe"
  File /oname=afsd.exe "${__FILEDIR__}\..\..\..\..\apps\desktop\src-tauri\windows\afsd.exe"
  File /oname=afs-cloud-files.exe "${__FILEDIR__}\..\..\..\..\apps\desktop\src-tauri\windows\afs-cloud-files.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro STOP_AFS_SIDECAR_PROCESSES
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\afs.exe"
  Delete "$INSTDIR\afsd.exe"
  Delete "$INSTDIR\afs-cloud-files.exe"
  DeleteRegValue HKCU "${AFS_RUN_KEY}" "${AFS_RUN_VALUE}"
  !insertmacro DELETE_AFS_TERMINAL_SHIM "$LOCALAPPDATA\Microsoft\WindowsApps\afs.cmd"
  !insertmacro DELETE_AFS_TERMINAL_SHIM "$LOCALAPPDATA\AgentFS\bin\afs.cmd"
  RMDir "$LOCALAPPDATA\AgentFS\bin"
!macroend
