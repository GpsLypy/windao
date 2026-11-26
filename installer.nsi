Unicode true
!include "MUI2.nsh"
!include "x64.nsh"

!define PRODUCT "WinDao"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDao"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define APP_MUTEX "Local\WinDao.Windows"

Name "${PRODUCT}"
OutFile "WinDao-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

!macro LockInstallation
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "${APP_MUTEX}") p .r0 ?e'
  Pop $1
  ${If} $1 == 183
    MessageBox MB_OK|MB_ICONEXCLAMATION "请先从托盘退出 WinDao，然后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${ElseIf} $0 == 0
    MessageBox MB_OK|MB_ICONSTOP "无法锁定安装，请稍后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
!macroend

Function .onInit
  ${IfNot} ${IsNativeAMD64}
    MessageBox MB_OK|MB_ICONSTOP "此版本需要 x64 Windows。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  !insertmacro LockInstallation
FunctionEnd

Section "安装"
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR"
  File "WinDao.exe"
  File "icon.ico"
  File "OFL-NotoSansSC.txt"
  File "LICENSE.txt"
  File "THIRD_PARTY_NOTICES.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\WinDao.exe" "" "$INSTDIR\icon.ico"
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
  Delete "$INSTDIR\WinDao.exe"
  ${If} ${Errors}
    MessageBox MB_OK|MB_ICONSTOP "无法移除 WinDao，请关闭正在占用它的程序后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  ReadRegStr $0 HKCU "${RUN_KEY}" "WinDao"
  ${If} $0 == '$\"$INSTDIR\WinDao.exe$\"'
    DeleteRegValue HKCU "${RUN_KEY}" "WinDao"
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
