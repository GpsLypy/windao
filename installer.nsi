Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

Var UpdateMode
Var InstallerMutex

!define PRODUCT "NotchCalendar"
!define APP_EXE "NotchCalendar.exe"
!define LEGACY_PRODUCT "WinDao"
!define LEGACY_EXE "WinDao.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDao"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define APP_MUTEX "Local\WinDao.Windows"

Name "${PRODUCT}"
OutFile "${PRODUCT}-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${LEGACY_PRODUCT}"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

VIProductVersion "${PRODUCT_FILE_VERSION}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "ProductName" "${PRODUCT}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "FileDescription" "${PRODUCT} 安装程序"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "FileVersion" "${PRODUCT_VERSION}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "LegalCopyright" ""

!macro LockInstallation
  StrCpy $2 0
  retry_lock:
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "${APP_MUTEX}") p .r0 ?e'
  Pop $1
  ${If} $1 == 183
    System::Call 'kernel32::CloseHandle(p r0)'
    ${If} $UpdateMode == "1"
    ${AndIf} $2 < 80
      IntOp $2 $2 + 1
      Sleep 250
      Goto retry_lock
    ${EndIf}
    MessageBox MB_OK|MB_ICONEXCLAMATION "请先从托盘退出 ${PRODUCT}，然后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${ElseIf} $0 == 0
    MessageBox MB_OK|MB_ICONSTOP "无法锁定安装，请稍后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  StrCpy $InstallerMutex $0
!macroend

Function .onInit
  ${IfNot} ${IsNativeAMD64}
    MessageBox MB_OK|MB_ICONSTOP "此版本需要 x64 Windows。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  SetRegView 64
  ReadRegStr $0 HKLM "Software\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${If} $0 < 19041
    MessageBox MB_OK|MB_ICONSTOP "此版本需要 Windows 10 版本 2004（内部版本 19041）或更高版本。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $1
  ${IfNot} ${Errors}
    StrCpy $UpdateMode "1"
    SetAutoClose true
    SetRegView 64
    ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
    ${If} $0 != ""
      StrCpy $INSTDIR $0
    ${EndIf}
  ${EndIf}
  !insertmacro LockInstallation
FunctionEnd

Function .onInstSuccess
  ${If} $UpdateMode == "1"
    System::Call 'kernel32::CloseHandle(p $InstallerMutex)'
    Exec '"$INSTDIR\${APP_EXE}"'
  ${EndIf}
FunctionEnd

Section "安装"
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR"
  File "${APP_EXE}"
  File "icon.ico"
  File "OFL-NotoSansSC.txt"
  File "LICENSE.txt"
  File "THIRD_PARTY_NOTICES.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\icon.ico"
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 == $INSTDIR
    IfFileExists "$INSTDIR\${LEGACY_EXE}" 0 legacy_exe_removed
    ClearErrors
    Delete "$INSTDIR\${LEGACY_EXE}"
    ${If} ${Errors}
      MessageBox MB_OK|MB_ICONSTOP "无法移除旧版，请关闭正在占用它的程序后重试。" /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
    legacy_exe_removed:
    Delete "$SMPROGRAMS\${LEGACY_PRODUCT}.lnk"
    ReadRegStr $0 HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}"
    ${If} $0 == '$\"$INSTDIR\${LEGACY_EXE}$\"'
      WriteRegStr HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}" '$\"$INSTDIR\${APP_EXE}$\"'
    ${EndIf}
  ${EndIf}
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\icon.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Function un.onInit
  !insertmacro LockInstallation
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  SetRegView 64
  ClearErrors
  Delete "$INSTDIR\${APP_EXE}"
  ${If} ${Errors}
    MessageBox MB_OK|MB_ICONSTOP "无法移除 ${PRODUCT}，请关闭正在占用它的程序后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  ReadRegStr $0 HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}"
  ${If} $0 == '$\"$INSTDIR\${APP_EXE}$\"'
    DeleteRegValue HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}"
  ${EndIf}
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\OFL-NotoSansSC.txt"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\Uninstall.exe"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  RMDir "$INSTDIR"
SectionEnd
