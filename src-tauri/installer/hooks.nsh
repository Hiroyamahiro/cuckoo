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
    MessageBox MB_OK|MB_ICONSTOP "別のアプリがあるため、このフォルダーにはインストールできません。" /SD IDOK
    SetErrorLevel 2
    Abort
  cuckoo_install_ok:
!macroend

!macro NSIS_HOOK_POSTINSTALL
  FileOpen $0 "$INSTDIR\.cuckoo-application-id" w
  FileWrite $0 "${BUNDLEID}"
  FileClose $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\.cuckoo-application-id"
  RMDir "$INSTDIR"
!macroend
