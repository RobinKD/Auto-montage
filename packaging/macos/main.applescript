-- Application Auto-montage (macOS). Copie le projet et le programme dans ~/Auto-montage au
-- premier lancement (packaging/macos/first-run.sh), puis propose les actions. L'interface est
-- lancée dans le Terminal (auto-montage start : installe le reste au premier lancement, puis
-- sert l'interface tant que la fenêtre reste ouverte et l'ouvre dans le navigateur).
on run
	set appPath to POSIX path of (path to me)
	set resources to appPath & "Contents/Resources/"
	try
		set dest to do shell script "bash " & quoted form of (resources & "first-run.sh") & " " & quoted form of (resources & "payload")
	on error errMsg
		display dialog "Installation impossible : " & errMsg buttons {"OK"} default button "OK" with title "Auto-montage" with icon stop
		return
	end try

	set choix to choose from list {"Moments et version de travail", "Téléchargements (720p, 1080p, 4K)", "Dossier des rushes", "Claude Code", "Arrêter Auto-montage"} with title "Auto-montage" with prompt "Que voulez-vous ouvrir ?" default items {"Moments et version de travail"} OK button name "Ouvrir" cancel button name "Annuler"
	if choix is false then return
	set c to item 1 of choix
	set montage to dest & "/montage"
	set am to quoted form of (montage & "/bin/auto-montage")
	if c starts with "Dossier" then
		set rushes to quoted form of (montage & "/public/rushes")
		do shell script "mkdir -p " & rushes & " && open " & rushes
		return
	else if c starts with "Moments" then
		set cmd to am & " start"
	else if c starts with "Téléchargements" then
		set cmd to am & " start downloads/"
	else if c is "Claude Code" then
		-- Claude Code de la machine, sinon celui installé par « auto-montage setup » (montage/claude, Node de montage/node).
		set cmd to "cd " & quoted form of montage & " && export PATH=" & quoted form of (montage & "/claude/node_modules/.bin:" & montage & "/node/bin") & ":\"$PATH\" && if command -v claude >/dev/null; then claude; else echo \"Claude Code n'est pas encore installé : ouvrez d'abord Auto-montage, qui l'installe au premier lancement.\"; fi"
	else
		set cmd to am & " stop"
	end if
	tell application "Terminal"
		activate
		do script cmd
	end tell
end run
