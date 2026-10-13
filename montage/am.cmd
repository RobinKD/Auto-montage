@echo off
rem Lance une commande d'Auto-montage sous Windows : am start, am prepare MonRush.mov…
rem Programme livré (bin\auto-montage.exe), sinon compilé ici avec Rust (cargo).
rem La commande tourne dans le dossier d'où elle est lancée (chemins donnés inchangés).
setlocal
set "D=%~dp0"
if exist "%D%bin\auto-montage.exe" (
  "%D%bin\auto-montage.exe" %*
  exit /b %errorlevel%
)
where cargo >nul 2>nul
if errorlevel 1 (
  echo Auto-montage n'est pas encore compile dans ce dossier, et Rust ^(cargo^) n'est pas installe. 1>&2
  echo - Pour l'utiliser simplement : prenez l'installateur de votre systeme sur la page des versions, 1>&2
  echo   https://github.com/RobinKD/Auto-montage/releases 1>&2
  echo - Pour le lancer depuis ce dossier : installez Rust ^(https://rustup.rs^), puis relancez cette 1>&2
  echo   commande ^(la premiere compilation prend quelques minutes^). 1>&2
  exit /b 1
)
cargo build --release --quiet --manifest-path "%D%rust\Cargo.toml" || exit /b 1
"%D%rust\target\release\auto-montage.exe" %*
exit /b %errorlevel%
