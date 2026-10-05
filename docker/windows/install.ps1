# Windows : raccourcis « Auto-montage » sur le bureau et dans le menu Démarrer.
# Usage : double-cliquer sur docker\install-windows.bat (Docker Desktop doit être installé).
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Icon = Join-Path $Root 'docker\icon.ico'
$Shell = New-Object -ComObject WScript.Shell
$Menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Auto-montage'
New-Item -ItemType Directory -Force -Path $Menu | Out-Null

function New-Shortcut([string]$Path, [string]$Script, [string]$Arguments, [string]$Description) {
    $link = $Shell.CreateShortcut($Path)
    $link.TargetPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $link.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $PSScriptRoot $Script)`" $Arguments".Trim()
    $link.WorkingDirectory = $Root
    $link.IconLocation = "$Icon,0"
    $link.Description = $Description
    $link.Save()
}

$items = @(
    @('Auto-montage', 'open.ps1', '', 'Version de travail, moments, corrections et effets'),
    @('Téléchargements', 'open.ps1', 'downloads/', 'Versions 720p, 1080p et 4K'),
    @('Dossier des rushes', 'rushes.ps1', '', 'Dossier où déposer les rushes'),
    @('Claude Code', 'claude.ps1', '', 'Claude Code dans le conteneur'),
    @('Terminal du conteneur', 'shell.ps1', '', 'Terminal dans le conteneur (dossier montage)'),
    @('Arrêter Auto-montage', 'stop.ps1', '', 'Arrête le conteneur')
)
foreach ($i in $items) {
    New-Shortcut (Join-Path $Menu "$($i[0]).lnk") $i[1] $i[2] $i[3]
}
New-Shortcut (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Auto-montage.lnk') 'open.ps1' '' 'Version de travail, moments, corrections et effets'
Write-Host "Raccourcis installés : bureau et menu Démarrer > Auto-montage."
