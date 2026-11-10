; Inno-Setup-Skript für MQTT Probe (Windows x64)
; Build:  ISCC.exe "X:\rust\mqtt-probe\packaging\windows\mqtt-probe.iss"
; WICHTIG: Alle Dateipfade sind absolute Pfade. Die .iss liegt im VirtualBox-Share (X:),
; dort loest ISCC relative Pfade nicht auf (Fehler 'I/O error 123' in der OutputDir-Zeile).
; Ebenfalls kein /O-Schalter verwenden - er kollidiert mit OutputDir.
; Die Version wird direkt aus der gebauten EXE gelesen.

#define MyAppName "MQTT Probe"
#define MyAppPublisher "MQTT Probe contributors"
#define MyAppURL "https://github.com/klumw/mqtt-probe"
#define MyAppExeName "mqtt-probe.exe"
#define MyAppId "{{B7E4A1C2-3D5F-4A8B-9C6E-1F2D3A4B5C6D}"
#define BuildExe "X:\rust\mqtt-probe\target\release\" + MyAppExeName
#define MyAppVersion ReadIni("X:\rust\mqtt-probe\Cargo.toml", "package", "version")

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
VersionInfoVersion={#MyAppVersion}
VersionInfoDescription={#MyAppName} Setup
VersionInfoCompany={#MyAppPublisher}
VersionInfoCopyright=Copyright (C) 2026 {#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
; Per-Benutzer-Installation: keine Admin-Rechte nötig.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
LicenseFile=X:\rust\mqtt-probe\LICENSE
SetupIconFile=X:\rust\mqtt-probe\assets\mqtt-probe.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
OutputDir=X:\rust\mqtt-probe\dist
OutputBaseFilename=MQTT-Probe-{#MyAppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no
SetupLogging=yes
MinVersion=10.0

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: {#BuildExe}; DestDir: "{app}"; Flags: ignoreversion
Source: "X:\rust\mqtt-probe\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "X:\rust\mqtt-probe\NOTICE"; DestDir: "{app}"; DestName: "NOTICE.txt"; Flags: ignoreversion
Source: "X:\rust\mqtt-probe\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"
Name: "{group}\Lizenz (Apache-2.0)"; Filename: "{app}\LICENSE.txt"
Name: "{group}\{#MyAppName} deinstallieren"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"
