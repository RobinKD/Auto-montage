# Démarre le conteneur si besoin et ouvre l'interface dans le navigateur.
# Usage : open.ps1 [downloads/]   (page des moments par défaut)
param([string]$Page = '')
. "$PSScriptRoot\common.ps1"
Wait-Docker
docker compose up -d --build
if ($LASTEXITCODE -ne 0) { Write-Host 'Le démarrage a échoué (voir les messages ci-dessus).'; Wait-Close; exit 1 }
# Serveur lancé avant une mise à jour (autre version que les fichiers) : redémarré.
$installed = if (Test-Path (Join-Path $Root 'VERSION')) { (Get-Content (Join-Path $Root 'VERSION') -Raw).Trim() } else { 'dev' }
$running = try { (Invoke-RestMethod -TimeoutSec 3 "http://localhost:$Port/api/version").version } catch { $null }
if ((Test-Ready) -and $running -ne $installed) {  # v0.1 : pas de /api/version, donc $null
    Write-Host "Mise à jour vers la version $installed : redémarrage d'Auto-montage…"
    docker compose up -d --force-recreate | Out-Null
    Start-Sleep -Seconds 2
}
if (-not (Test-Ready)) {
    Write-Host "Démarrage d'Auto-montage. La première fois, l'installation prend 10 à 20 min."
    $logs = Start-Process docker -ArgumentList 'compose', 'logs', '-f', '--tail', '20', 'app' -NoNewWindow -PassThru
    while (-not (Test-Ready)) {
        $running = docker compose ps --status running -q app
        if (-not $running) {
            Stop-Process -Id $logs.Id -ErrorAction SilentlyContinue
            Write-Host "Le conteneur s'est arrêté : voir « docker compose logs app »."
            Wait-Close
            exit 1
        }
        Start-Sleep -Seconds 3
    }
    Stop-Process -Id $logs.Id -ErrorAction SilentlyContinue
}
$url = "http://localhost:$Port/$Page"
Write-Host "Interface prête : $url"
Start-Process $url
