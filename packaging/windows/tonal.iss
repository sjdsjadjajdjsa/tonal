; Windows installer (Inno Setup 6). Installs for the current user only, so it
; needs no admin rights. Built by .github/workflows/release.yml:
;
;   iscc /DVersion=0.1.0 /DSource=C:\full\path\to\dist\tonal packaging\windows\tonal.iss
;
; mpv.exe and yt-dlp.exe ship next to tonal.exe. The library and settings live
; in %APPDATA%\tonal, so they stay on uninstall.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Source
  #define Source "..\..\dist\tonal"
#endif

[Setup]
AppId={{6F2C8E41-93B7-4D5A-A1E0-7C4B9D2F8E15}
AppName=Tonal
AppVersion={#Version}
AppVerName=Tonal {#Version}
AppPublisher=cj
AppCopyright=cj, MIT
VersionInfoDescription=Tonal installer
AppSupportURL=https://github.com/sjdsjadjajdjsa/tonal
AppUpdatesURL=https://github.com/sjdsjadjajdjsa/tonal/releases
DefaultDirName={userpf}\Tonal
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile={#Source}\LICENSE
SetupIconFile=tonal.ico
UninstallDisplayIcon={app}\tonal.exe
UninstallDisplayName=Tonal
OutputDir=..\..
OutputBaseFilename=tonal-setup-x86_64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "{#Source}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{userprograms}\Tonal"; Filename: "{app}\tonal.exe"; WorkingDir: "{app}"; Comment: "Music player"
Name: "{userdesktop}\Tonal"; Filename: "{app}\tonal.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\tonal.exe"; Description: "Open Tonal"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent
