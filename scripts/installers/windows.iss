#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef ReleaseVersion
  #error ReleaseVersion is required
#endif

[Setup]
AppId={{B1FD8B3A-76DE-4DF8-96B2-502E5F10C99F}
AppName=Atelier Kanban
AppVersion={#ReleaseVersion}
AppPublisher=Atelier Kanban
DefaultDirName={localappdata}\Programs\AtelierKanban
DefaultGroupName=Atelier Kanban
PrivilegesRequired=lowest
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableDirPage=yes
DisableProgramGroupPage=yes
CloseApplications=no
RestartApplications=no
UninstallDisplayIcon={app}\AtelierKanban.exe
UninstallDisplayName=Atelier Kanban
SetupIconFile={#SourceDir}\public\atelier-kanban.ico
OutputDir={#OutputDir}
OutputBaseFilename=atelier-kanban-{#ReleaseVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autodesktop}\Atelier Kanban"; Filename: "{app}\AtelierKanban.exe"; WorkingDir: "{app}"; Tasks: desktopicon
Name: "{group}\Atelier Kanban"; Filename: "{app}\AtelierKanban.exe"
Name: "{group}\Arrêter Atelier Kanban"; Filename: "{app}\AtelierKanban.exe"; Parameters: "--stop"
Name: "{group}\Importer les données portables"; Filename: "{app}\AtelierKanban.exe"; Parameters: "--import-data"
Name: "{group}\Désinstaller Atelier Kanban"; Filename: "{uninstallexe}"

[Run]
; Explorer keeps the installer's RedirectionGuard from blocking user-created agent/worktree junctions.
Filename: "{win}\explorer.exe"; Parameters: """{app}\AtelierKanban.exe"""; Description: "Ouvrir Atelier Kanban"; Flags: postinstall nowait skipifsilent unchecked

[Code]
function StopAtelier(): Boolean;
var ResultCode: Integer;
begin
  Result := True;
  if FileExists(ExpandConstant('{app}\runtime\node.exe')) then
    Result := Exec(ExpandConstant('{app}\runtime\node.exe'), '"' + ExpandConstant('{app}\launcher.mjs') + '" --stop', ExpandConstant('{app}'), SW_HIDE, ewWaitUntilTerminated, ResultCode) and (ResultCode = 0);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not StopAtelier() then
    Result := 'Arrêtez les agents, les chronomètres et Atelier avant de poursuivre l’installation.';
end;

function InitializeUninstall(): Boolean;
begin
  Result := StopAtelier();
  if not Result then
    MsgBox('Arrêtez les opérations en cours dans Atelier avant la désinstallation. Les données personnelles sont conservées.', mbError, MB_OK);
end;
