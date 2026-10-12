@echo off
rem Lance une commande d'Auto-montage sous Windows : am prepare MonRush.mov, am local_server…
rem Programme livré (bin\auto-montage.exe), sinon compilé ici avec Rust (cargo).
rem La commande tourne dans le dossier d'où elle est lancée (chemins donnés inchangés).
setlocal
set "D=%~dp0"
if exist "%D%bin\auto-montage.exe" (
  "%D%bin\auto-montage.exe" %*
  exit /b %errorlevel%
)
cargo build --release --quiet --manifest-path "%D%rust\Cargo.toml" || exit /b 1
"%D%rust\target\release\auto-montage.exe" %*
exit /b %errorlevel%
