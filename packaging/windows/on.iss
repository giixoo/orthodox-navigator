; Inno Setup script for the Windows installer (built by .github/workflows/release.yml):
;   ISCC.exe /DAppVersion=0.1.0 packaging\windows\on.iss
; Installs for the current user (no admin rights needed; the installer offers all users too).

#ifndef AppVersion
  #define AppVersion "0.3.0"
#endif

[Setup]
AppId={{6F0B3F0E-6C1D-4E43-9A57-0D1B2B8C4A11}
AppName=Orthodox Navigator
AppVersion={#AppVersion}
AppPublisher=Orthodox Navigator
AppPublisherURL=https://github.com/giixoo/orthodox-navigator
DefaultDirName={autopf}\Orthodox Navigator
DefaultGroupName=Orthodox Navigator
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
LicenseFile=..\..\LICENSE
OutputBaseFilename=on-{#AppVersion}-windows-x64-setup
UninstallDisplayIcon={app}\bin\on.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "path"; Description: "Add the program to PATH"
Name: "profile"; Description: "Add the ""on"" command to your PowerShell profiles (recommended: commands then run in your shell, and cd is kept)"

[Files]
Source: "..\..\rust\target\release\on.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "..\..\shell\on.ps1"; DestDir: "{app}\shell"; Flags: ignoreversion
Source: "add-to-profile.ps1"; DestDir: "{app}\shell"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Orthodox Navigator"; Filename: "{app}\bin\on.exe"; WorkingDir: "{userdocs}"
Name: "{group}\Uninstall Orthodox Navigator"; Filename: "{uninstallexe}"

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}\bin"; Tasks: path; Check: (not IsAdminInstallMode) and NeedsPath(ExpandConstant('{app}\bin'))
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}\bin"; Tasks: path; Check: IsAdminInstallMode and NeedsPath(ExpandConstant('{app}\bin'))

[Run]
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\shell\add-to-profile.ps1"" ""{app}\shell\on.ps1"""; Tasks: profile; Flags: runhidden
Filename: "{app}\bin\on.exe"; Description: "Start Orthodox Navigator"; WorkingDir: "{userdocs}"; Flags: postinstall nowait skipifsilent unchecked

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\shell\add-to-profile.ps1"" ""{app}\shell\on.ps1"" -Remove"; Flags: runhidden; RunOnceId: "RemoveProfileLine"

[Code]
function EnvRoot: Integer;
begin
  if IsAdminInstallMode then Result := HKEY_LOCAL_MACHINE else Result := HKEY_CURRENT_USER;
end;

function EnvKey: String;
begin
  if IsAdminInstallMode then Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else Result := 'Environment';
end;

function NeedsPath(Dir: String): Boolean;
var
  Path: String;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', Path) then
    Result := True
  else
    Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') = 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Path, Dir: String;
  P: Integer;
begin
  if CurUninstallStep <> usPostUninstall then Exit;
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', Path) then Exit;
  Dir := ';' + ExpandConstant('{app}\bin');
  P := Pos(Uppercase(Dir), Uppercase(Path));
  if P > 0 then
  begin
    Delete(Path, P, Length(Dir));
    RegWriteExpandStringValue(EnvRoot, EnvKey, 'Path', Path);
  end;
end;
