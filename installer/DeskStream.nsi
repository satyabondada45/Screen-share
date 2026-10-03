!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!ifndef DESKSTREAM_SIGNING_SCRIPT
!ifndef DESKSTREAM_DEVELOPMENT_BUILD
!error "Pass a production signing script or explicitly mark this as a development build."
!endif
!endif

!ifndef DESKSTREAM_OUTFILE
!define DESKSTREAM_OUTFILE "..\dist-installer\DeskStream-Setup-x64.exe"
!endif

Var VerifyPayloadDir

Name "DeskStream"
OutFile "${DESKSTREAM_OUTFILE}"
InstallDir "$LOCALAPPDATA\DeskStream"
RequestExecutionLevel user

!define MUI_ABORTWARNING
!define MUI_ICON "..\desktop-agent\assets\icon.ico"
!define MUI_UNICON "..\desktop-agent\assets\icon.ico"
!define MUI_HEADERIMAGE_RIGHT

Icon "..\desktop-agent\assets\icon.ico"
UninstallIcon "..\desktop-agent\assets\icon.ico"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\DeskStream.exe"
!insertmacro MUI_PAGE_FINISH

!ifdef DESKSTREAM_SIGNING_SCRIPT
!uninstfinalize 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "${DESKSTREAM_SIGNING_SCRIPT}" -ExePath "%1"' = 0
!endif

!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${GetParameters} $R0
  ${GetOptions} $R0 "/VERIFY-PAYLOAD=" $R1
  ${If} $R1 == "1"
    ReadEnvStr $VerifyPayloadDir "DESKSTREAM_VERIFY_DIR"
    ${If} $VerifyPayloadDir == ""
      Abort
    ${EndIf}
    SetSilent silent
  ${EndIf}
FunctionEnd

Section "DeskStream (required)"
  SectionIn RO

  ${If} $VerifyPayloadDir != ""
    SetOutPath "$VerifyPayloadDir"
    File "..\DESKSTREAM\DeskStream.exe"
    SetErrorLevel 0
    Quit
  ${EndIf}
  
  SetOutPath "$INSTDIR"
  
  ; Include the executable built by Cargo
  File "..\DESKSTREAM\DeskStream.exe"
  
  ; Copy icon to use for shortcuts if it exists
  IfFileExists "..\desktop-agent\assets\icon.ico" 0 +2
  File "..\desktop-agent\assets\icon.ico"
  
  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"
  
  ; Create Desktop Shortcut
  CreateShortcut "$DESKTOP\DeskStream.lnk" "$INSTDIR\DeskStream.exe" "" "$INSTDIR\icon.ico" 0
  
  ; Create Start Menu Shortcut
  CreateDirectory "$SMPROGRAMS\DeskStream"
  CreateShortcut "$SMPROGRAMS\DeskStream\DeskStream.lnk" "$INSTDIR\DeskStream.exe" "" "$INSTDIR\icon.ico" 0
  
  ; Write uninstall registry keys for Add/Remove Programs
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DeskStream" "DisplayName" "DeskStream"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DeskStream" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DeskStream" "DisplayIcon" '"$INSTDIR\icon.ico"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DeskStream" "Publisher" "DeskStream"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DeskStream" "DisplayVersion" "1.1.3"
SectionEnd

Section "Uninstall"
  ; Remove shortcuts
  Delete "$DESKTOP\DeskStream.lnk"
  Delete "$SMPROGRAMS\DeskStream\DeskStream.lnk"
  RMDir "$SMPROGRAMS\DeskStream"
  
  ; Remove executable and icon
  Delete "$INSTDIR\DeskStream.exe"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\uninstall.exe"
  
  ; Remove registry keys
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\DeskStream"
  
  ; We specifically DO NOT delete user data, logs, or configs in $INSTDIR
  ; Only if it's explicitly chosen, but for now we leave it.
  
SectionEnd
