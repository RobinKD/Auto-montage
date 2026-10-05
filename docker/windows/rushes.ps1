# Ouvre le dossier où déposer les rushes (montage\public\rushes).
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Dir = Join-Path $Root 'montage\public\rushes'
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
Start-Process explorer.exe $Dir
