# Commun aux lanceurs Windows : dossier du dépôt, port, Docker démarré.
# 'Continue' : sous Windows PowerShell 5.1, un message de docker sur stderr arrêterait sinon le script.
$ErrorActionPreference = 'Continue'
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $Root
$Port = if ($env:AM_PORT) { $env:AM_PORT } else { '8080' }
# Docker Desktop se charge des droits sur les fichiers : identifiants fixes dans le conteneur.
if (-not $env:AM_UID) { $env:AM_UID = '1000' }
if (-not $env:AM_GID) { $env:AM_GID = '1000' }

# Pause avant de fermer la fenêtre (sauf dans les tests automatiques : variable CI).
function Wait-Close([string]$Message = 'Appuyez sur Entrée pour fermer') {
    if (-not $env:CI) { Read-Host $Message | Out-Null }
}

function Wait-Docker {
    docker info *> $null
    if ($LASTEXITCODE -eq 0) {
        # Docker Desktop en mode « conteneurs Windows » : Auto-montage a besoin des conteneurs Linux.
        $os = docker info --format '{{.OSType}}' 2>$null
        if ($os -and $os -ne 'linux') {
            Write-Host 'Docker Desktop est en mode « conteneurs Windows ». Clic droit sur son icône (à côté de l''horloge) > « Switch to Linux containers… », puis relancez Auto-montage.'
            Wait-Close
            exit 1
        }
        return
    }
    $desktop = Join-Path $env:ProgramFiles 'Docker\Docker\Docker Desktop.exe'
    if (Test-Path $desktop) {
        Write-Host 'Démarrage de Docker Desktop…'
        Start-Process $desktop
        do { Start-Sleep -Seconds 3; docker info *> $null } until ($LASTEXITCODE -eq 0)
    } else {
        Write-Host 'Docker ne répond pas : installez ou démarrez Docker Desktop, puis relancez.'
        Wait-Close
        exit 1
    }
}

function Test-Ready {
    try {
        Invoke-WebRequest -UseBasicParsing -TimeoutSec 3 "http://localhost:$Port/api/regen" | Out-Null
        return $true
    } catch { return $false }
}
