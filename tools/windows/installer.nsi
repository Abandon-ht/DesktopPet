Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"

Name "DesktopPet ${APP_VERSION}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_ID}"
InstallDirRegKey HKCU "Software\${APP_ID}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 32
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey "ProductName" "DesktopPet"
VIAddVersionKey "FileDescription" "DesktopPet Windows x64 安装程序"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "DesktopPet contributors"

!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "安装你的桌面伙伴"
!define MUI_WELCOMEPAGE_TEXT "安装包已包含 DesktopPet、角色、互动语音和本地语音模型，无需分别下载资源压缩包。$\r$\n$\r$\n安装到当前用户，无需管理员权限。升级前请先从托盘退出旧程序。$\r$\n$\r$\n语音对话仍需另行配置 Ollama 或其他 LLM 服务。首次安装缺少 WebView2 时需要联网下载微软运行时。"
!define MUI_COMPONENTSPAGE_TEXT_TOP "选择快捷方式和开机自启。安装后也可在应用的启动设置里修改自启。"
!define MUI_FINISHPAGE_RUN "$INSTDIR\desktop-pet.exe"
!define MUI_FINISHPAGE_RUN_TEXT "启动 DesktopPet"
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!define MUI_FINISHPAGE_TEXT "安装完成。使用桌面或开始菜单中的 DesktopPet 启动，程序不会显示终端窗口。$\r$\n$\r$\n设置与退出入口位于系统托盘。开机自启可在应用设置里开启或关闭。"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "DesktopPet 程序和全部资源（必需）" Application
  SectionIn RO
  SetShellVarContext current
  SetRegView 64
  ; @INSTALL_FILES@
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\${APP_ID}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "DisplayName" "DesktopPet"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "DisplayIcon" "$INSTDIR\desktop-pet.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "NoRepair" 1
  CreateDirectory "$SMPROGRAMS\${APP_ID}"
  CreateShortcut "$SMPROGRAMS\${APP_ID}\DesktopPet.lnk" "$INSTDIR\desktop-pet.exe"
  CreateShortcut "$SMPROGRAMS\${APP_ID}\卸载 DesktopPet.lnk" "$INSTDIR\Uninstall.exe"
SectionEnd

Section "创建桌面快捷方式" DesktopShortcut
  CreateShortcut "$DESKTOP\${APP_ID}.lnk" "$INSTDIR\desktop-pet.exe"
SectionEnd

Section /o "登录 Windows 时自动启动" Startup
SectionEnd

Section -Finish
  ${If} ${SectionIsSelected} ${Startup}
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}" '$\"$INSTDIR\desktop-pet.exe$\"'
  ${Else}
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${EndIf}
  !ifdef WEBVIEW2_BOOTSTRAPPER
    ; Microsoft documents the EdgeUpdate Clients key in the 32-bit registry view.
    SetRegView 32
    ReadRegStr $0 HKLM "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
    ReadRegStr $1 HKCU "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
    ${If} $0 == "0.0.0.0"
      StrCpy $0 ""
    ${EndIf}
    ${If} $1 == "0.0.0.0"
      StrCpy $1 ""
    ${EndIf}
    ${If} $0 == ""
    ${AndIf} $1 == ""
      InitPluginsDir
      SetOutPath "$PLUGINSDIR"
      File /oname=WebView2Setup.exe "${WEBVIEW2_BOOTSTRAPPER}"
      ExecWait '$\"$PLUGINSDIR\WebView2Setup.exe$\" /silent /install' $0
      ${If} $0 != 0
        MessageBox MB_OK|MB_ICONSTOP "WebView2 安装失败，请联网后重新运行安装程序。" /SD IDOK
        SetErrorLevel 1
        Quit
      ${EndIf}
    ${EndIf}
    SetRegView 64
  !endif
SectionEnd

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "DesktopPet 需要 Windows 10/11 x64。" /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  SetShellVarContext current
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $0 != ""
    SectionSetFlags ${Startup} ${SF_SELECTED}
  ${EndIf}
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/AUTOSTART=" $1
  ${IfNot} ${Errors}
    ${If} $1 == "1"
      SectionSetFlags ${Startup} ${SF_SELECTED}
    ${Else}
      SectionSetFlags ${Startup} 0
    ${EndIf}
  ${EndIf}
FunctionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext current
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $0 == '$\"$INSTDIR\desktop-pet.exe$\"'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${EndIf}
  Delete "$DESKTOP\${APP_ID}.lnk"
  Delete "$SMPROGRAMS\${APP_ID}\DesktopPet.lnk"
  Delete "$SMPROGRAMS\${APP_ID}\卸载 DesktopPet.lnk"
  RMDir "$SMPROGRAMS\${APP_ID}"
  ; Delete only the generated list of application files; preserve user additions.
  ; @UNINSTALL_FILES@
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  ReadRegStr $0 HKCU "Software\${APP_ID}" "InstallDir"
  ${If} $0 == $INSTDIR
    DeleteRegKey HKCU "Software\${APP_ID}"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
  ${EndIf}
  ; Character libraries, settings and databases live in app data and are retained.
SectionEnd
