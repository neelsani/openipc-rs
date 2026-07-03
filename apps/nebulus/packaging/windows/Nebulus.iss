#ifndef AppVersion
  #error AppVersion must be provided by CI
#endif
#ifndef SourceDir
  #error SourceDir must be provided by CI
#endif
#ifndef RepoDir
  #error RepoDir must be provided by CI
#endif
#ifndef Architecture
  #error Architecture must be provided by CI
#endif
#ifndef OutputDir
  #error OutputDir must be provided by CI
#endif
#ifndef OutputName
  #error OutputName must be provided by CI
#endif

[Setup]
AppId={{D13015DD-A730-4FD7-A5FC-A1E30D491D72}
AppName=Nebulus
AppVersion={#AppVersion}
AppPublisher=openipc-rs
AppPublisherURL=https://github.com/neelsani/openipc-rs
AppSupportURL=https://openipc-rs.neels.dev
AppUpdatesURL=https://github.com/neelsani/openipc-rs/releases
DefaultDirName={autopf}\Nebulus
DefaultGroupName=Nebulus
DisableProgramGroupPage=yes
LicenseFile={#RepoDir}\LICENSE
ArchitecturesAllowed={#Architecture}
ArchitecturesInstallIn64BitMode={#Architecture}
PrivilegesRequired=admin
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\Nebulus.exe
CloseApplications=yes
RestartApplications=no

[Files]
Source: "{#SourceDir}\nebulus.exe"; DestDir: "{app}"; DestName: "Nebulus.exe"; Flags: ignoreversion
Source: "{#SourceDir}\wintun.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoDir}\apps\nebulus\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Nebulus"; Filename: "{app}\Nebulus.exe"
Name: "{autodesktop}\Nebulus"; Filename: "{app}\Nebulus.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Run]
Filename: "{app}\Nebulus.exe"; Description: "Launch Nebulus"; Flags: nowait postinstall skipifsilent
