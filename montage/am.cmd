@echo off
rem Lance une commande d'Auto-montage sous Windows : am prepare MonRush.mov, am local_server…
rem Programme livré (bin\auto-montage.exe), sinon compilé ici avec Rust (cargo).
setlocal
cd /d "%~dp0"
if exist bin\auto-montage.exe (
  bin\auto-montage.exe %*
  exit /b %errorlevel%
)
cargo build --release --quiet --manifest-path rust\Cargo.toml || exit /b 1
rust\target\release\auto-montage.exe %*
exit /b %errorlevel%
