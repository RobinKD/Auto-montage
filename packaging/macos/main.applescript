-- Application Auto-montage (macOS). Copie le projet dans ~/Auto-montage au premier lancement
-- (packaging/macos/first-run.sh), vérifie Docker Desktop, puis propose les actions.
on run
	set appPath to POSIX path of (path to me)
	set resources to appPath & "Contents/Resources/"
	try
		set dest to do shell script "bash " & quoted form of (resources & "first-run.sh") & " " & quoted form of (resources & "payload")
	on error errMsg
		display dialog "Installation impossible : " & errMsg buttons {"OK"} default button "OK" with title "Auto-montage" with icon stop
		return
	end try

	set dockerPath to "PATH=/usr/local/bin:/opt/homebrew/bin:/Applications/Docker.app/Contents/Resources/bin:$PATH; "
	try
		do shell script dockerPath & "command -v docker"
	on error
		set reponse to display dialog "Auto-montage a besoin de Docker Desktop (gratuit pour un usage personnel), qui n'est pas installé." & return & return & "Installez-le depuis la page qui va s'ouvrir, lancez-le une fois, puis rouvrez Auto-montage." buttons {"Annuler", "Télécharger Docker Desktop"} default button 2 with title "Auto-montage" with icon caution
		if button returned of reponse is "Télécharger Docker Desktop" then open location "https://www.docker.com/products/docker-desktop/"
		return
	end try

	set choix to choose from list {"Moments et version de travail", "Téléchargements (720p, 1080p, 4K)", "Dossier des rushes", "Claude Code", "Terminal du conteneur", "Arrêter Auto-montage"} with title "Auto-montage" with prompt "Que voulez-vous ouvrir ?" default items {"Moments et version de travail"} OK button name "Ouvrir" cancel button name "Annuler"
	if choix is false then return
	set c to item 1 of choix
	set docker to quoted form of (dest & "/docker")
	if c starts with "Dossier" then
		do shell script "bash " & docker & "/rushes.sh"
		return
	else if c starts with "Moments" then
		set cmd to docker & "/open.sh"
	else if c starts with "Téléchargements" then
		set cmd to docker & "/open.sh downloads/"
	else if c is "Claude Code" then
		set cmd to docker & "/claude.sh"
	else if c starts with "Terminal" then
		set cmd to docker & "/shell.sh"
	else
		set cmd to docker & "/stop.sh"
	end if
	tell application "Terminal"
		activate
		do script dockerPath & cmd
	end tell
end run
