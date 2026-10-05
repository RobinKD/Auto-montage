# Après l'installation (Windows) : vérifie Docker Desktop, propose de l'installer s'il manque,
# puis lance Auto-montage.
. "$PSScriptRoot\common.ps1"
Add-Type -AssemblyName System.Windows.Forms

function Ask([string]$Text) {
    [System.Windows.Forms.MessageBox]::Show($Text, 'Auto-montage', 'YesNo', 'Question') -eq 'Yes'
}
function Tell([string]$Text) {
    [System.Windows.Forms.MessageBox]::Show($Text, 'Auto-montage', 'OK', 'Information') | Out-Null
}

$desktop = Join-Path $env:ProgramFiles 'Docker\Docker\Docker Desktop.exe'
$hasDocker = (Get-Command docker -ErrorAction SilentlyContinue) -or (Test-Path $desktop)
if (-not $hasDocker) {
    $msg = "Auto-montage a besoin de Docker Desktop (gratuit pour un usage personnel), qui n'est pas installé.`n`nL'installer maintenant ? Windows demandera l'autorisation, puis il faudra redémarrer l'ordinateur."
    if (-not (Ask $msg)) {
        Tell "Installez Docker Desktop depuis https://www.docker.com/products/docker-desktop/ puis lancez Auto-montage depuis le bureau."
        exit 0
    }
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        winget install -e --id Docker.DockerDesktop --accept-source-agreements --accept-package-agreements
        if ($LASTEXITCODE -eq 0) {
            Tell "Docker Desktop est installé. Redémarrez l'ordinateur, ouvrez Docker Desktop une fois (accepter les conditions), puis lancez Auto-montage depuis le bureau."
            exit 0
        }
    }
    Start-Process 'https://www.docker.com/products/docker-desktop/'
    Tell "Téléchargez et installez Docker Desktop depuis la page qui vient de s'ouvrir, redémarrez, puis lancez Auto-montage depuis le bureau."
    exit 0
}

& "$PSScriptRoot\open.ps1"
