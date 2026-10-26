#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef ReleaseVersion
  #error ReleaseVersion is required
#endif
[Setup]
AppId={{1E47A40D-620A-4815-AED7-B4CB019B4BB9}
AppName=Atelier Kanban Rust
AppVersion={#ReleaseVersion}
AppPublisher=Atelier Kanban
DefaultDirName={localappdata}\Programs\AtelierKanbanRust
DefaultGroupName=Atelier Kanban Rust
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
DisableDirPage=yes
DisableProgramGroupPage=yes
CloseApplications=no
RestartApplications=no
AppMutex=Global\AtelierKanbanRustRunning
SetupMutex=Global\AtelierKanbanRustSetup
UninstallDisplayIcon={app}\atelier.exe
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
Name: "{group}\Atelier Kanban Rust"; Filename: "{app}\atelier.exe"; WorkingDir: "{app}"
Name: "{group}\Arrêter Atelier Kanban Rust"; Filename: "{app}\atelier.exe"; Parameters: "stop"
Name: "{group}\Désinstaller Atelier Kanban Rust"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Atelier Kanban Rust"; Filename: "{app}\atelier.exe"; WorkingDir: "{app}"; Tasks: desktopicon
[Code]
function ReserveMaintenance(): Boolean;
begin
  CreateMutex('Global\AtelierKanbanRustMaintenance');
  Result := not CheckForMutexes('Global\AtelierKanbanRustRunning');
  if not Result then
    MsgBox('Arrêtez toutes les instances Atelier Rust avant de poursuivre. / Stop all Atelier Rust instances first.', mbError, MB_OK);
end;
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not ReserveMaintenance() then
    Result := 'Atelier Rust est encore ouvert. / Atelier Rust is still running.'
  else if DirExists(ExpandConstant('{app}')) then
    Result := 'Installation existante conservée. Utilisez la mise à jour signée ou désinstallez-la après vérification. / Existing installation preserved. Use signed updates or uninstall it after verification.';
end;
function InitializeUninstall(): Boolean;
var Code: Integer;
begin
  Result := ReserveMaintenance();
  if Result then
    Result := Exec(ExpandConstant('{app}\atelier.exe'), 'verify-installation', ExpandConstant('{app}'), SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
  if not Result then
    MsgBox('Désinstallation refusée : instance ouverte, code modifié ou historique à examiner. Aucun effacement forcé. / Uninstall refused: running instance, modified code or history requiring review. No forced deletion.', mbError, MB_OK);
end;
