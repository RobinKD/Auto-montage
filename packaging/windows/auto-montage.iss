; Installateur Windows d'Auto-montage (Inno Setup 6).
; Construit par .github/workflows/installateurs.yml :
;   ISCC /DAppVersion=0.1 /DPayload=<projet> /DOutDir=<sortie> packaging\windows\auto-montage.iss
; Installation pour l'utilisateur seul (pas de droits administrateur) dans Documents\Auto-montage :
; les rushes, rendus et sélections y restent après une mise à jour ou une désinstallation.

#ifndef AppVersion
  #define AppVersion "0.0"
#endif
#ifndef Payload
  #define Payload "..\..\build\payload"
#endif
#ifndef OutDir
  #define OutDir "..\..\build\dist"
#endif

[Setup]
AppId={{6E4B3B6A-1C1B-4F63-9B5B-6A2F0D7C5E21}
AppName=Auto-montage
AppVersion={#AppVersion}
AppVerName=Auto-montage {#AppVersion}
AppPublisher=Auto-montage
AppPublisherURL=https://github.com/RobinKD/Auto-montage
DefaultDirName={userdocs}\Auto-montage
DisableDirPage=no
DefaultGroupName=Auto-montage
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutDir}
OutputBaseFilename=Auto-montage-{#AppVersion}-Windows-installateur
SetupIconFile={#Payload}\docker\icon.ico
UninstallDisplayIcon={app}\docker\icon.ico
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
ShowLanguageDialog=no

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Files]
Source: "{#Payload}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Dirs]
Name: "{app}\montage\public\rushes"; Flags: uninsneveruninstall

[Icons]
Name: "{group}\Auto-montage"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\open.ps1"""; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Version de travail, moments, corrections et effets"
Name: "{group}\Téléchargements"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\open.ps1"" downloads/"; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Versions 720p, 1080p et 4K"
Name: "{group}\Dossier des rushes"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ""{app}\docker\windows\rushes.ps1"""; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Dossier où déposer les rushes"
Name: "{group}\Claude Code"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\claude.ps1"""; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Claude Code dans le conteneur"
Name: "{group}\Terminal du conteneur"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\shell.ps1"""; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Terminal dans le conteneur (dossier montage)"
Name: "{group}\Nettoyer Docker"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\clean.ps1"""; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Libère la place prise par les anciennes images de Docker"
Name: "{group}\Arrêter Auto-montage"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\stop.ps1"""; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Arrête le conteneur"
Name: "{group}\Désinstaller Auto-montage"; Filename: "{uninstallexe}"
Name: "{userdesktop}\Auto-montage"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\open.ps1"""; WorkingDir: "{app}"; IconFilename: "{app}\docker\icon.ico"; Comment: "Version de travail, moments, corrections et effets"

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\docker\windows\first-run.ps1"""; WorkingDir: "{app}"; Description: "Vérifier Docker Desktop et lancer Auto-montage"; Flags: postinstall nowait skipifsilent
