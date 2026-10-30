; build.ps1 compiles this twice: the full installer, and with /DUpdateOnly the slim update
; installer the app downloads, which has no tools and only updates an existing installation.
#ifndef AppVersion
  #error AppVersion must be supplied by build.ps1
#endif
#ifndef PortableDir
  #define PortableDir "..\dist\stream-dlp"
#endif

#ifndef NumericVersion
  #define NumericVersion AppVersion
#endif
#ifndef ReleaseOutput
  #define ReleaseOutput "..\dist"
#endif

[Setup]
AppId={{2B8804F9-9B57-472E-B8E0-B97062EEACB9}
AppName=stream-dlp
AppVersion={#AppVersion}
VersionInfoVersion={#NumericVersion}
AppPublisher=Puffinnois
AppPublisherURL=https://github.com/Puffinnois/stream-dlp
AppUpdatesURL=https://github.com/Puffinnois/stream-dlp/releases
DefaultDirName={localappdata}\Programs\stream-dlp
DefaultGroupName=stream-dlp
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#ReleaseOutput}
#ifdef UpdateOnly
OutputBaseFilename=stream-dlp-Update-Windows-x64
DisableDirPage=yes
#else
OutputBaseFilename=stream-dlp-Setup-Windows-x64
DisableDirPage=no
#endif
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\stream-dlp.exe
LicenseFile=..\LICENSE
WizardStyle=modern
Compression=lzma2/fast
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
DisableProgramGroupPage=yes

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[InstallDelete]
; The docs are replaced as a whole, so moved or removed ones do not stay behind.
Type: filesandordirs; Name: "{app}\docs"

[Files]
Source: "{#PortableDir}\stream-dlp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\Setup tools.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\docs\*"; DestDir: "{app}\docs"; Flags: ignoreversion recursesubdirs
#ifndef UpdateOnly
; Explicit helper list avoids packaging build tools stored under tools/.
Source: "{#PortableDir}\tools\yt-dlp.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\ffmpeg.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\ffprobe.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\deno.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\versions.json"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\licenses\*"; DestDir: "{app}\tools\licenses"; Flags: ignoreversion recursesubdirs
#endif

[Icons]
Name: "{autoprograms}\stream-dlp"; Filename: "{app}\stream-dlp.exe"
Name: "{autodesktop}\stream-dlp"; Filename: "{app}\stream-dlp.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\stream-dlp.exe"; Description: "Open stream-dlp"; Flags: nowait postinstall skipifsilent
; The app's silent update passes /RELAUNCH=1 to reopen it; other silent installs (CI) do not.
Filename: "{app}\stream-dlp.exe"; Flags: nowait skipifnotsilent; Check: Relaunch

[Code]
function Relaunch: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;
#ifdef UpdateOnly

// Without the tools, the update cannot be a first installation.
function InitializeSetup: Boolean;
begin
  Result := RegKeyExists(HKA, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{2B8804F9-9B57-472E-B8E0-B97062EEACB9}_is1');
  if not Result then
    SuppressibleMsgBox('This file only updates an installed stream-dlp. To install stream-dlp, download stream-dlp-Setup-Windows-x64.exe from the releases page.', mbError, MB_OK, IDOK);
end;
#endif
