"""Avancement des étapes longues, affiché en barre de progression par l'interface locale.

Les scripts écrivent sur leur sortie des lignes « @progression <fait> <total> <libellé> » ;
scripts/local_server.py les lit (sans les mettre au journal) et les pages les affichent. Dans
un terminal, ces lignes restent lisibles telles quelles.

En Python : from progress import report ; report("Transcription", 12.5, 300)
En ligne de commande, un encodage ffmpeg suivi pas à pas (durée de l'entrée lue d'abord) :
  python3 scripts/progress.py ffmpeg "<libellé>" <ffmpeg> <arguments…>
"""
import re
import subprocess
import sys
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


def ffmpeg(label, cmd, total=None):
    """Lance ffmpeg avec -progress et rapporte le temps encodé sur la durée de l'entrée (ou sur
    « total » secondes de sortie, pour une sortie plus courte que l'entrée)."""
    if total is None:
        inputs = [cmd[i + 1] for i, a in enumerate(cmd[:-1]) if a == "-i"]
        probe = subprocess.run([cmd[0], "-hide_banner", "-i", inputs[0]], capture_output=True, text=True) if inputs else None
        m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", probe.stderr if probe else "")
        total = _seconds(*m.groups()) if m else 0
    if total:
        report(label, 0, total)  # la barre (et son chronomètre) démarre avec ffmpeg
    proc = subprocess.Popen(cmd[:1] + ["-progress", "pipe:1", "-nostats"] + cmd[1:],
                            stdout=subprocess.PIPE, text=True)
    for line in proc.stdout:
        key, _, value = line.strip().partition("=")
        if key == "out_time_us" and total and value.isdigit():
            report(label, min(int(value) / 1e6, total), total)
    code = proc.wait()
    if code == 0 and total:
        report(label, total, total)
    return code


if __name__ == "__main__":
    if len(sys.argv) < 4 or sys.argv[1] != "ffmpeg":
        sys.exit(__doc__)
    sys.exit(ffmpeg(sys.argv[2], sys.argv[3:]))
