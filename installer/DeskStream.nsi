!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "DeskStream"
OutFile "..\dist-installer\DeskStream-Setup-x64.exe"
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

!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "DeskStream (required)"
  SectionIn RO
  
  SetOutPath "$INSTDIR"
  
  ; Include the executable built by Cargo
  File "..\desktop-agent\target\release\DeskStream.exe"
  
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
