; Inno Setup script for the Windows installer (x64). Built by packaging/windows/build-installer.ps1,
; which passes the version: ISCC /DMyAppVersion=0.1.0 mathcraft.iss
; Inno Setup is a free tool (not shipped). The installer contains only Mathcraft's own files.

#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppName "Mathcraft"
#define MyAppExe "mathcraft-app.exe"

[Setup]
; Fixed GUID: upgrades and uninstalls depend on it. Never change it.
AppId={{6F1D2C7A-3B8E-4E0A-9C51-7A2B5D4E8F10}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=Mathcraft contributors
DefaultDirName={autopf}\Mathcraft
DefaultGroupName=Mathcraft
DisableProgramGroupPage=yes
OutputDir=..\..\target\packaging
OutputBaseFilename=Mathcraft-{#MyAppVersion}-windows-x64-setup
SetupIconFile=..\..\crates\mathcraft-app\assets\icons\mathcraft.ico
UninstallDisplayIcon={app}\mathcraft.ico
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
ChangesAssociations=yes
; The optional PATH entry below changes the environment; Setup then tells running programs.
ChangesEnvironment=yes
; The licence page shows the MIT text; the Apache-2.0 text ships in {app}\licenses.
LicenseFile=..\..\LICENSE-MIT
VersionInfoVersion={#MyAppVersion}
VersionInfoProductName={#MyAppName}

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "addtopath"; Description: "Add mathcraft-mcp to the system PATH (lets AI clients run it by name; open a new terminal afterwards)"; GroupDescription: "AI assistants (MCP):"; Flags: unchecked

[Files]
Source: "..\..\target\release\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
; The MCP server for AI clients (documentation/MCP_USER_GUIDE.md). Built with the render feature.
Source: "..\..\target\release\mathcraft-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\crates\mathcraft-app\assets\icons\mathcraft.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\..\NOTICE"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\..\documentation\THIRD_PARTY_LICENSES.md"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\..\crates\mathcraft-app\assets\fonts\LICENSES\*.txt"; DestDir: "{app}\licenses\fonts"; Flags: ignoreversion

[Icons]
Name: "{group}\Mathcraft"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\mathcraft.ico"
Name: "{group}\Uninstall Mathcraft"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Mathcraft"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\mathcraft.ico"; Tasks: desktopicon

[Registry]
; .mcdx file association (HKA = HKLM for an admin install). The ProgID is removed on uninstall.
Root: HKA; Subkey: "Software\Classes\.mcdx"; ValueType: string; ValueName: ""; ValueData: "Mathcraft.Worksheet"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Mathcraft.Worksheet"; ValueType: string; ValueName: ""; ValueData: "Mathcad Prime worksheet package"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Mathcraft.Worksheet\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\mathcraft.ico"
Root: HKA; Subkey: "Software\Classes\Mathcraft.Worksheet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" ""%1"""

[Registry]
; Optional PATH entry (task addtopath). Adds {app} once; uninstall removes it again (RemoveFromPath).
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath('{app}')

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "Launch Mathcraft"; Flags: nowait postinstall skipifsilent unchecked

[Code]
// The machine PATH key, read by NeedsAddPath and RemoveFromPath.
const
  EnvKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

// True when {app} is not yet an entry of the machine PATH.
function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvKey, 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Param) + ';', ';' + Uppercase(OrigPath) + ';') = 0;
end;

// Removes the {app} entry that the installer added (uninstall only).
procedure RemoveFromPath();
var
  OrigPath, Wrapped, AppDir: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvKey, 'Path', OrigPath) then exit;
  AppDir := ExpandConstant('{app}');
  // Match whole entries only (a sibling such as "<app> Tools" must stay): wrap the list in
  // semicolons, replace ";<app>;" by ";", then strip the added semicolons again.
  Wrapped := ';' + OrigPath + ';';
  if StringChangeEx(Wrapped, ';' + AppDir + ';', ';', True) > 0 then
  begin
    if (Length(Wrapped) > 0) and (Wrapped[1] = ';') then Delete(Wrapped, 1, 1);
    if (Length(Wrapped) > 0) and (Wrapped[Length(Wrapped)] = ';') then
      Delete(Wrapped, Length(Wrapped), 1);
    RegWriteExpandStringValue(HKEY_LOCAL_MACHINE, EnvKey, 'Path', Wrapped);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then RemoveFromPath();
end;
