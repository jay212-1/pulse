#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{F45C2342-CE61-4077-B055-CF4D0535C478}
AppName=Pulse
AppVersion={#AppVersion}
AppPublisher=Cheter
DefaultDirName={autopf}\Showtime Suite\Pulse
DefaultGroupName=Pulse
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=Pulse-Setup-{#AppVersion}
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\pulse.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\stage\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Pulse"; Filename: "{app}\pulse.exe"
Name: "{autodesktop}\Pulse"; Filename: "{app}\pulse.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.pulse"; ValueType: string; ValueData: "Pulse.Profile"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Pulse.Profile"; ValueType: string; ValueData: "Pulse profile"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Pulse.Profile\DefaultIcon"; ValueType: string; ValueData: "{app}\pulse.exe,0"
Root: HKA; Subkey: "Software\Classes\Pulse.Profile\shell\open\command"; ValueType: string; ValueData: """{app}\pulse.exe"" ""%1"""

[Run]
Filename: "{app}\pulse.exe"; Description: "{cm:LaunchProgram,Pulse}"; Flags: nowait postinstall
