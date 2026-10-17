!define CUCKOO_MAINTENANCE "${__FILEDIR__}\..\..\setup\build\CuckooMaintenance.exe"

; Do not overwrite an Electron or unrelated installation, even for a custom /D.
!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$INSTDIR\Update.exe" cuckoo_conflict
  IfFileExists "$INSTDIR\Cuckoo.exe" 0 cuckoo_install_ok
  ClearErrors
  FileOpen $0 "$INSTDIR\.cuckoo-application-id" r
  IfErrors cuckoo_conflict
  FileRead $0 $1
  FileClose $0
  StrCmp $1 "${BUNDLEID}" cuckoo_install_ok cuckoo_conflict
  cuckoo_conflict:
    SetErrorLevel 2
    Abort
  cuckoo_install_ok:
!macroend

!macro NSIS_HOOK_POSTINSTALL
  FileOpen $0 "$INSTDIR\.cuckoo-application-id" w
  FileWrite $0 "${BUNDLEID}"
  FileClose $0
  SetOutPath "$INSTDIR"
  File /oname=CuckooMaintenance.exe "${CUCKOO_MAINTENANCE}"
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" '$\"$INSTDIR\CuckooMaintenance.exe$\" /uninstall'
  WriteRegStr SHCTX "${UNINSTKEY}" "QuietUninstallString" '$\"$INSTDIR\CuckooMaintenance.exe$\" /uninstall /quiet'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\CuckooMaintenance.exe"
  Delete "$INSTDIR\.cuckoo-application-id"
  RMDir "$INSTDIR"
!macroend

; Fail closed instead of forcibly terminating the application in silent mode.
!macro CuckooRequireStopped
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro RestartManager_StartSession $R0
  ${If} $R0 == ""
    SetErrorLevel 25
    Abort
  ${EndIf}
  !insertmacro RestartManager_RegisterFile $R0 "$INSTDIR\${MAINBINARYNAME}.exe"
  ${If} $0 <> 0
    !insertmacro RestartManager_EndSession $R0
    SetErrorLevel 25
    Abort
  ${EndIf}
  StrCpy $1 0
  StrCpy $2 0
  System::Call 'RSTRTMGR::RmGetList(p R0, *i .r1, *i .r2, p 0, *i .r3) i .r0'
  StrCpy $R1 $0
  !insertmacro RestartManager_EndSession $R0
  ${If} $R1 <> 0
    SetErrorLevel 24
    Abort
  ${EndIf}
  ${EndIf}
!macroend
