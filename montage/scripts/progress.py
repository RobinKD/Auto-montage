"""Avancement des étapes longues, affiché en barre de progression par l'interface locale.

Les scripts écrivent sur leur sortie des lignes « @progression <fait> <total> <libellé> » ;
scripts/local_server.py les lit (sans les mettre au journal) et les pages les affichent. Dans
un terminal, ces lignes restent lisibles telles quelles.

En Python : from progress import report ; report("Transcription", 12.5, 300)
En ligne de commande, un encodage ffmpeg suivi pas à pas (durée de l'entrée lue d'abord) :
  python3 scripts/progress.py ffmpeg "<libellé>" <ffmpeg> <arguments…>
"""
import collections
import os
import re
import shutil
import signal
import subprocess
import sys
import threading
import time

_last = {}


def report(label, done, total, every=0.5):
    """Écrit l'avancement, au plus toutes les « every » secondes par libellé (et toujours à la fin)."""
    now = time.monotonic()
    if done < total and now - _last.get(label, 0) < every:
        return
    _last[label] = now
    print(f"@progression {done:.6g} {total:.6g} {label}", flush=True)


def _seconds(h, m, s):
    return int(h) * 3600 + int(m) * 60 + float(s)


def ffmpeg(label, cmd, total=None, offset=0.0, whole=None):
    """Lance ffmpeg avec -progress et rapporte le temps encodé sur la durée de l'entrée (ou sur
    « total » secondes de sortie, pour une sortie plus courte que l'entrée). Un morceau d'un
    travail plus long (offset, whole) : rapporté comme offset + temps encodé sur whole."""
    if total is None:
        inputs = [cmd[i + 1] for i, a in enumerate(cmd[:-1]) if a == "-i"]
        probe = subprocess.run([cmd[0], "-hide_banner", "-i", inputs[0]], capture_output=True, text=True) if inputs else None
        m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", probe.stderr if probe else "")
        total = _seconds(*m.groups()) if m else 0
    whole = whole or total
    if total:
        report(label, offset, whole)  # la barre (et son chronomètre) démarre avec ffmpeg
    # Messages de ffmpeg recopiés au fil de l'eau (journal) et gardés pour le bilan d'un échec.
    proc = subprocess.Popen(cmd[:1] + ["-progress", "pipe:1", "-nostats"] + cmd[1:],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, errors="replace")
    tail = collections.deque(maxlen=40)

    def relay():
        for line in proc.stderr:
            tail.append(line.rstrip())
            print(line.rstrip(), flush=True)
    reader = threading.Thread(target=relay, daemon=True)
    reader.start()
    for line in proc.stdout:
        key, _, value = line.strip().partition("=")
        if key == "out_time_us" and total and value.isdigit():
            report(label, offset + min(int(value) / 1e6, total), whole)
    code = proc.wait()
    reader.join(5)
    if code == 0 and total:
        report(label, offset + total, whole)
    if code:
        print(f"ERREUR : ffmpeg ({label}) s'est arrêté : {exit_reason(code)}", flush=True)
        print(f"   {resources(cmd[-1])}", flush=True)
        if tail:
            print("   Derniers messages de ffmpeg :", *(f"   | {l}" for l in tail), sep="\n", flush=True)
        else:
            print("   ffmpeg n'a écrit aucun message d'erreur.", flush=True)
    return 128 - code if code < 0 else code  # tué par un signal : code 128 + n, comme le shell


def exit_reason(code, shell=False):
    """Explication d'un code de sortie : négatif (processus lancé par Python) ou, avec shell, de
    129 à 159 (script shell dont une commande a été tuée) : arrêté par un signal."""
    sig = -code if code < 0 else code - 128 if shell and 128 < code < 160 else 0
    if not sig:
        return f"code {code}"
    try:
        name = signal.Signals(sig).name
    except ValueError:
        name = str(sig)
    why = {"SIGKILL": "tué de force, le plus souvent par manque de mémoire (Docker : Réglages > Resources)",
           "SIGTERM": "arrêté (annulation, arrêt ou redémarrage du conteneur)",
           "SIGSEGV": "plantage du programme", "SIGBUS": "plantage (disque plein ou fichier modifié pendant la lecture ?)",
           "SIGINT": "interrompu", "SIGHUP": "terminal fermé", "SIGPIPE": "sortie fermée"}.get(name, "")
    return (f"code {code}, " if code > 0 else "") + f"signal {name}" + (f" : {why}" if why else "")


def resources(path="."):
    """Place libre sur le disque de « path » et mémoire disponible, pour le journal."""
    out = []
    try:
        folder = os.path.dirname(os.path.abspath(path)) or "."
        out.append(f"disque libre {shutil.disk_usage(folder).free / 2**30:.1f} Go")
    except OSError:
        pass
    try:
        info = dict(l.split(":", 1) for l in open("/proc/meminfo"))
        avail, total = (int(info[k].split()[0]) / 2**20 for k in ("MemAvailable", "MemTotal"))
        out.append(f"mémoire disponible {avail:.1f} Go sur {total:.1f} Go")
    except (OSError, KeyError, ValueError):
        pass
    return ", ".join(out) or "ressources inconnues"

if __name__ == "__main__":
    if len(sys.argv) < 4 or sys.argv[1] != "ffmpeg":
        sys.exit(__doc__)
    sys.exit(ffmpeg(sys.argv[2], sys.argv[3:]))
