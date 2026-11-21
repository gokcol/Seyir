#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef ExecutablePath
  #error ExecutablePath is required
#endif

[Setup]
AppId=io.github.gokcol.seyir
AppName=Seyir
AppVersion={#AppVersion}
AppPublisher=gokcol
AppPublisherURL=https://github.com/gokcol/Seyir
AppSupportURL=https://github.com/gokcol/Seyir/issues
AppUpdatesURL=https://github.com/gokcol/Seyir/releases
VersionInfoVersion={#AppVersion}.0
VersionInfoProductName=Seyir
VersionInfoDescription=Seyir Setup
DefaultDirName={localappdata}\Programs\Seyir
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=Seyir.ico
UninstallDisplayIcon={app}\seyir.exe
WizardStyle=modern
Compression=lzma2
SolidCompression=yes

[Files]
Source: "{#ExecutablePath}"; DestDir: "{app}"; DestName: "seyir.exe"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Seyir"; Filename: "{app}\seyir.exe"

[Run]
Filename: "{app}\seyir.exe"; Parameters: "--register-open-with"; Flags: runhidden waituntilterminated
Filename: "{app}\seyir.exe"; Description: "{cm:LaunchProgram,Seyir}"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command, Executable: String;
  ResultCode: Integer;
begin
  if CurUninstallStep = usUninstall then begin
    Executable := ExpandConstant('{app}\seyir.exe');
    if RegQueryStringValue(HKCU, 'Software\Classes\Seyir.Image.1\shell\open\command', '', Command) and
       (CompareText(Command, '"' + Executable + '" "%1"') = 0) then begin
      if not Exec(Executable, '--unregister-open-with', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) or
         (ResultCode <> 0) then
        Log('Could not remove the installed copy from Explorer Open with.');
    end;
  end;
end;
