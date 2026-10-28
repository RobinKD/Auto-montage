; Installateur Windows d'Auto-montage (Inno Setup 6).
; Construit par .github/workflows/installateurs.yml :
;   ISCC /DAppVersion=0.1 /DPayload=<projet> /DOutDir=<sortie> packaging\windows\auto-montage.iss
; Le projet (Payload, make-payload.sh) contient le programme Windows : montage\bin\auto-montage.exe.
; Installation pour l'utilisateur seul (pas de droits administrateur) dans Documents\Auto-montage :
; les rushes, rendus et sélections y restent après une mise à jour ou une désinstallation.
; Les raccourcis lancent le programme (auto-montage.exe start : installe le reste au premier
; lancement, sert l'interface et l'ouvre dans le navigateur, tant que sa fenêtre est ouverte).

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
SetupIconFile={#Payload}\icons\icon.ico
UninstallDisplayIcon={app}\icons\icon.ico
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
Name: "{group}\Auto-montage"; Filename: "{app}\montage\bin\auto-montage.exe"; Parameters: "start"; WorkingDir: "{app}\montage"; IconFilename: "{app}\icons\icon.ico"; Comment: "Version de travail, moments, corrections et effets"
Name: "{group}\Téléchargements"; Filename: "{app}\montage\bin\auto-montage.exe"; Parameters: "start downloads/"; WorkingDir: "{app}\montage"; IconFilename: "{app}\icons\icon.ico"; Comment: "Versions 720p, 1080p et 4K"
Name: "{group}\Dossier des rushes"; Filename: "{win}\explorer.exe"; Parameters: """{app}\montage\public\rushes"""; IconFilename: "{app}\icons\icon.ico"; Comment: "Dossier où déposer les rushes"
Name: "{group}\Claude Code"; Filename: "{cmd}"; Parameters: "/K set ""PATH={app}\montage\claude\node_modules\.bin;{app}\montage\node;%PATH%"" & where claude >nul 2>&1 & if errorlevel 1 (echo Claude Code n'est pas encore installé : ouvrez d'abord Auto-montage, qui l'installe au premier lancement.) else claude"; WorkingDir: "{app}\montage"; IconFilename: "{app}\icons\icon.ico"; Comment: "Claude Code dans le dossier d'Auto-montage"
Name: "{group}\Arrêter Auto-montage"; Filename: "{cmd}"; Parameters: "/C """"{app}\montage\bin\auto-montage.exe"" stop & timeout /t 5 >nul"""; WorkingDir: "{app}\montage"; IconFilename: "{app}\icons\icon.ico"; Comment: "Arrête l'interface"
Name: "{group}\Désinstaller Auto-montage"; Filename: "{uninstallexe}"
Name: "{userdesktop}\Auto-montage"; Filename: "{app}\montage\bin\auto-montage.exe"; Parameters: "start"; WorkingDir: "{app}\montage"; IconFilename: "{app}\icons\icon.ico"; Comment: "Version de travail, moments, corrections et effets"

[Run]
Filename: "{app}\montage\bin\auto-montage.exe"; Parameters: "start"; WorkingDir: "{app}\montage"; Description: "Lancer Auto-montage (la première fois : 10 à 20 min d'installation)"; Flags: postinstall nowait skipifsilent
