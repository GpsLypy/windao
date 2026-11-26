Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"
!include "nsDialogs.nsh"
!include "Win\COM.nsh"

Var UpdateMode
Var InstallerMutex
Var PreviousLocation
Var DesktopChoice
Var LoginChoice
Var DesktopCheckbox
Var LoginCheckbox
Var LinkFile
Var LinkTarget

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
!define MUI_PAGE_CUSTOMFUNCTION_PRE DirectoryPre
!insertmacro MUI_PAGE_DIRECTORY
Page custom OptionsPage OptionsLeave
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

!macro ReadLink PREFIX
Function ${PREFIX}ReadLink
  StrCpy $LinkTarget ""
  IfFileExists "$LinkFile" 0 link_done
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r5 ".r8"
  ${If} $8 == 0
    ${IUnknown::QueryInterface} $5 '("${IID_IPersistFile}",.r6).r8'
    ${If} $8 == 0
      ${IPersistFile::Load} $6 '("$LinkFile",0).r8'
      ${If} $8 == 0
        ${IShellLink::GetPath} $5 '(.r7,${NSIS_MAX_STRLEN},0,4).r8'
        ${If} $8 == 0
          StrCpy $LinkTarget $7
        ${EndIf}
      ${EndIf}
      ${IUnknown::Release} $6 ""
    ${EndIf}
    ${IUnknown::Release} $5 ""
  ${EndIf}
  link_done:
FunctionEnd
!macroend
!insertmacro ReadLink ""
!insertmacro ReadLink "un."

!macro ManageShortcut PREFIX LINK CHOICE
  StrCpy $LinkFile "${LINK}"
  Call ${PREFIX}ReadLink
  ${If} $LinkTarget == "$INSTDIR\${APP_EXE}"
  ${OrIf} $LinkTarget == "$INSTDIR\${LEGACY_EXE}"
    Delete "$LinkFile"
  ${EndIf}
  ${If} "${CHOICE}" == "1"
  ${AndIfNot} ${FileExists} "$LinkFile"
    CreateShortCut "$LinkFile" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
  ${EndIf}
!macroend

Function DirectoryPre
  ${If} $PreviousLocation != ""
    Abort
  ${EndIf}
FunctionEnd

Function OptionsPage
  !insertmacro MUI_HEADER_TEXT "安装选项" "选择桌面入口和登录时的行为。"
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateCheckbox} 0 8u 100% 12u "创建桌面快捷方式"
  Pop $DesktopCheckbox
  ${NSD_SetState} $DesktopCheckbox $DesktopChoice
  ${NSD_CreateCheckbox} 0 30u 100% 12u "登录 Windows 时启动"
  Pop $LoginCheckbox
  ${NSD_SetState} $LoginCheckbox $LoginChoice
  nsDialogs::Show
FunctionEnd

Function OptionsLeave
  ${NSD_GetState} $DesktopCheckbox $DesktopChoice
  ${NSD_GetState} $LoginCheckbox $LoginChoice
FunctionEnd

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
    SetSilent silent
    SetAutoClose true
  ${EndIf}
  SetShellVarContext current
  ReadRegStr $PreviousLocation HKCU "${UNINSTALL_KEY}" "InstallLocation"
  StrCpy $DesktopChoice "1"
  StrCpy $LoginChoice "0"
  ${If} $PreviousLocation != ""
    StrCpy $INSTDIR $PreviousLocation
    StrCpy $DesktopChoice "0"
    StrCpy $LinkFile "$DESKTOP\${PRODUCT}.lnk"
    Call ReadLink
    ${If} $LinkTarget == "$INSTDIR\${APP_EXE}"
    ${OrIf} $LinkTarget == "$INSTDIR\${LEGACY_EXE}"
      StrCpy $DesktopChoice "1"
    ${EndIf}
    ReadRegStr $0 HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}"
    ${If} $0 == '"$INSTDIR\${APP_EXE}"'
    ${OrIf} $0 == '"$INSTDIR\${LEGACY_EXE}"'
      StrCpy $LoginChoice "1"
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
  ${If} $PreviousLocation == ""
    ${If} ${FileExists} "$INSTDIR\${APP_EXE}"
    ${OrIf} ${FileExists} "$INSTDIR\Uninstall.exe"
    ${OrIf} ${FileExists} "$INSTDIR\icon.ico"
    ${OrIf} ${FileExists} "$INSTDIR\LICENSE.txt"
    ${OrIf} ${FileExists} "$INSTDIR\OFL-NotoSansSC.txt"
    ${OrIf} ${FileExists} "$INSTDIR\THIRD_PARTY_NOTICES.txt"
      MessageBox MB_OK|MB_ICONSTOP "该目录已有同名文件，请选择其他安装目录。" /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
  SetOutPath "$INSTDIR"
  File "${APP_EXE}"
  File "icon.ico"
  File "OFL-NotoSansSC.txt"
  File "LICENSE.txt"
  File "THIRD_PARTY_NOTICES.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  !insertmacro ManageShortcut "" "$SMPROGRAMS\${PRODUCT}.lnk" "1"
  !insertmacro ManageShortcut "" "$DESKTOP\${PRODUCT}.lnk" "$DesktopChoice"
  ReadRegStr $0 HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}"
  ${If} $0 == '"$INSTDIR\${APP_EXE}"'
  ${OrIf} $0 == '"$INSTDIR\${LEGACY_EXE}"'
    DeleteRegValue HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}"
    StrCpy $0 ""
  ${EndIf}
  ${If} $LoginChoice == "1"
  ${AndIf} $0 == ""
    WriteRegStr HKCU "${RUN_KEY}" "${LEGACY_PRODUCT}" '"$INSTDIR\${APP_EXE}"'
  ${EndIf}
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
    !insertmacro ManageShortcut "" "$SMPROGRAMS\${LEGACY_PRODUCT}.lnk" "0"
  ${EndIf}
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '$\"$INSTDIR\${APP_EXE}$\",0'
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
  !insertmacro ManageShortcut "un." "$SMPROGRAMS\${PRODUCT}.lnk" "0"
  !insertmacro ManageShortcut "un." "$DESKTOP\${PRODUCT}.lnk" "0"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\OFL-NotoSansSC.txt"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\Uninstall.exe"
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 == "$INSTDIR"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ${EndIf}
  RMDir "$INSTDIR"
SectionEnd
