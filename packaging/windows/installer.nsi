Unicode true
RequestExecutionLevel admin
!include "MUI2.nsh"
!include "x64.nsh"

!ifndef PAYLOAD_DIR
  !error "PAYLOAD_DIR is required"
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE is required"
!endif
!ifndef PRODUCT_VERSION
  !error "PRODUCT_VERSION is required"
!endif
!ifndef TARGET_ARCH
  !define TARGET_ARCH "x64"
!endif
!ifndef BUILD_VERSION
  !define BUILD_VERSION "${PRODUCT_VERSION}.0"
!endif
!ifndef DISPLAY_VERSION
  !define DISPLAY_VERSION "${PRODUCT_VERSION}"
!endif

Name "GP Connect"
OutFile "${OUTPUT_FILE}"
InstallDir "$PROGRAMFILES64\GP Connect"
VIProductVersion "${BUILD_VERSION}"
VIAddVersionKey "ProductName" "GP Connect"
VIAddVersionKey "FileDescription" "GP Connect Installer"
VIAddVersionKey "ProductVersion" "${DISPLAY_VERSION}"
VIAddVersionKey "FileVersion" "${BUILD_VERSION}"
VIAddVersionKey "LegalCopyright" "GP Connect contributors"
ShowInstDetails show
ShowUninstDetails show
!define MUI_ICON "${__FILEDIR__}\..\..\apps\gpauth\icons\windows\icon.ico"
!define MUI_UNICON "${MUI_ICON}"
!define MUI_WELCOMEFINISHPAGE_BITMAP "${__FILEDIR__}\sidebar.bmp"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_TITLE "GP Connect is ready"
!define MUI_FINISHPAGE_TEXT "Open GP Connect from the Start menu."
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
!if "${TARGET_ARCH}" == "arm64"
  ${IfNot} ${IsNativeARM64}
    MessageBox MB_OK|MB_ICONSTOP "This GP Connect installer requires Windows 11 on ARM64." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
!else if "${TARGET_ARCH}" == "x64"
  ${IfNot} ${IsNativeAMD64}
    MessageBox MB_OK|MB_ICONSTOP "This GP Connect installer requires x64 Windows." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
!else
  !error "Unsupported TARGET_ARCH"
!endif
  SetRegView 64
FunctionEnd

Section "Install"
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\payload"
  File /r "${PAYLOAD_DIR}\*"
  WriteUninstaller "$PLUGINSDIR\payload\uninstall.exe"
  SetOutPath "$PLUGINSDIR"
  File "${__FILEDIR__}\machine-install.ps1"
  DetailPrint "Preparing GP Connect and waiting for open windows to close..."
  nsExec::ExecToLog '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\machine-install.ps1" -Action Install -SourceDirectory "$PLUGINSDIR\payload" -Version "${PRODUCT_VERSION}" -ConsumeSource'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "GP Connect could not be installed. See the details below and try again." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
SectionEnd

Section "Uninstall"
  SetRegView 64
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "${__FILEDIR__}\machine-install.ps1"
  DetailPrint "Waiting for GP Connect and sign-in windows to close..."
  nsExec::ExecToLog '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\machine-install.ps1" -Action Uninstall'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "GP Connect could not be removed. See the details below and try again." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
SectionEnd
