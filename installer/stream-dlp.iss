#ifndef AppVersion
  #error AppVersion must be supplied by build.ps1
#endif
#ifndef PortableDir
  #define PortableDir "..\dist\stream-dlp"
#endif

#ifndef AppLabel
  #define AppLabel "stream-dlp"
#endif
#ifndef NumericVersion
  #define NumericVersion AppVersion
#endif
#ifndef ReleaseOutput
  #define ReleaseOutput "..\dist"
#endif

[Setup]
AppId={{2B8804F9-9B57-472E-B8E0-B97062EEACB9}
AppName={#AppLabel}
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
OutputBaseFilename=stream-dlp-Setup-Windows-x64
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\stream-dlp.exe
LicenseFile=..\LICENSE
WizardStyle=modern
Compression=lzma2/fast
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
DisableProgramGroupPage=yes
DisableDirPage=no

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#PortableDir}\stream-dlp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\Setup tools.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PortableDir}\docs\*"; DestDir: "{app}\docs"; Flags: ignoreversion recursesubdirs
; Explicit helper list avoids packaging build tools stored under tools/.
Source: "{#PortableDir}\tools\yt-dlp.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\ffmpeg.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\ffprobe.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\deno.exe"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\versions.json"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#PortableDir}\tools\licenses\*"; DestDir: "{app}\tools\licenses"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\stream-dlp"; Filename: "{app}\stream-dlp.exe"
Name: "{autodesktop}\stream-dlp"; Filename: "{app}\stream-dlp.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\stream-dlp.exe"; Description: "Open stream-dlp"; Flags: nowait postinstall skipifsilent
