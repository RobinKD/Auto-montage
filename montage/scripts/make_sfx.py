"""Synthétise des effets sonores provisoires dans public/sfx/.

Ils peuvent être remplacés par de vrais fichiers en gardant les mêmes noms :
bop.wav, key-1.wav..key-4.wav, cash.wav, et les sons courants des vidéos courtes :
whoosh, pop, ding, boum, montee, glitch, photo, faux, juste (liste : moments_lib.SOUNDS).
"""
import os
import wave

import numpy as np

SR = 48000
OUT = os.path.join(os.path.dirname(__file__), "..", "public", "sfx")
os.makedirs(OUT, exist_ok=True)
rng = np.random.default_rng(7)


def save(name, x):
    path = os.path.join(OUT, name)
    if os.path.exists(path):  # ne jamais écraser un vrai bruitage déposé à la main
        return
    x = x / (np.max(np.abs(x)) + 1e-9) * 0.9
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes((x * 32767).astype(np.int16).tobytes())


def t(dur):
    return np.arange(int(SR * dur)) / SR


# "Bop" : sinus court dont la hauteur descend, attaque douce.
tt = t(0.14)
freq = 900 * np.exp(-tt * 18) + 260
phase = 2 * np.pi * np.cumsum(freq) / SR
env = np.minimum(1, tt / 0.004) * np.exp(-tt * 28)
save("bop.wav", np.sin(phase) * env)

# Frappe clavier : bruit filtré très court + petit "thock" grave.
for i in range(1, 5):
    tt = t(0.06)
    noise = rng.standard_normal(len(tt))
    noise = np.convolve(noise, np.ones(3) / 3, mode="same")
    click = noise * np.exp(-tt * (260 + 40 * i))
    thock = np.sin(2 * np.pi * (140 + 25 * i) * tt) * np.exp(-tt * 90) * 0.6
    save(f"key-{i}.wav", click + thock)

# Machine à cash : "ka" mécanique puis "ching" (cloche inharmonique).
ka = t(0.08)
ka_sig = rng.standard_normal(len(ka)) * np.exp(-ka * 70) * 0.5
ching = t(1.1)
partials = [(2093, 1.0), (2637, 0.6), (3136, 0.45), (4186, 0.3), (5274, 0.2)]
bell = sum(a * np.sin(2 * np.pi * f * ching) * np.exp(-ching * (3 + f / 1500)) for f, a in partials)
bell *= np.minimum(1, ching / 0.002)
gap = np.zeros(int(SR * 0.05))
save("cash.wav", np.concatenate([ka_sig, gap, bell * 0.8]))


def bandpass(x, f0, q=2.0):
    """Passe-bande (biquad) à fréquence centrale variable (tableau f0, un par échantillon)."""
    y = np.zeros_like(x)
    x1 = x2 = y1 = y2 = 0.0
    for n in range(len(x)):
        w = 2 * np.pi * f0[n] / SR
        alpha = np.sin(w) / (2 * q)
        cw = np.cos(w)
        a0 = 1 + alpha
        out = (alpha * x[n] - alpha * x2 - (-2 * cw) * y1 - (1 - alpha) * y2) / a0
        x2, x1 = x1, x[n]
        y2, y1 = y1, out
        y[n] = out
    return y


# Whoosh : souffle dont la bande monte puis redescend (passage rapide, transition).
tt = t(0.6)
shape = np.sin(np.pi * tt / 0.6) ** 2
save("whoosh.wav", bandpass(rng.standard_normal(len(tt)), 500 + 2500 * shape, 1.5) * shape)

# Pop : bulle qui éclate (sinus très court qui chute, petit clic d'attaque).
tt = t(0.09)
freq = 1300 * np.exp(-tt * 45) + 300
pop = np.sin(2 * np.pi * np.cumsum(freq) / SR) * np.exp(-tt * 55)
pop[:60] += rng.standard_normal(60) * 0.4
save("pop.wav", pop)

# Ding : petite cloche claire (bonne idée, conseil).
tt = t(1.3)
partials = [(1568, 1.0), (3136, 0.35), (4704, 0.15), (2350, 0.2)]
ding = sum(a * np.sin(2 * np.pi * f * tt) * np.exp(-tt * (2.5 + f / 2500)) for f, a in partials)
save("ding.wav", ding * np.minimum(1, tt / 0.003))

# Boum : impact grave (moment fort), pitch qui tombe et souffle étouffé.
tt = t(1.1)
freq = 110 * np.exp(-tt * 6) + 42
body = np.tanh(2.2 * np.sin(2 * np.pi * np.cumsum(freq) / SR)) * np.exp(-tt * 3.2)
thud = np.convolve(rng.standard_normal(len(tt)), np.ones(40) / 40, mode="same") * np.exp(-tt * 25) * 1.5
save("boum.wav", (body + thud) * np.minimum(1, tt / 0.004))

# Montée : tension qui monte (souffle et ton qui grimpent), coupée net à la fin.
tt = t(1.6)
rise = (tt / 1.6) ** 2
tone = np.sin(2 * np.pi * np.cumsum(180 + 1000 * rise) / SR) * 0.35
air = bandpass(rng.standard_normal(len(tt)), 400 + 4000 * rise, 3.0)
mont = (tone + air) * rise
mont[-200:] *= np.linspace(1, 0, 200)
save("montee.wav", mont)

# Glitch : coupures numériques (morceaux de signaux carrés et de bruit, hachés).
parts = []
for k in range(9):
    d = rng.uniform(0.02, 0.05)
    tt = t(d)
    if k % 3 == 2:
        parts.append(np.zeros(len(tt)))
        continue
    sq = np.sign(np.sin(2 * np.pi * rng.uniform(200, 1800) * tt))
    nz = np.round(rng.standard_normal(len(tt)) * 3) / 3
    parts.append((sq * 0.6 + nz * 0.4) * rng.uniform(0.5, 1))
save("glitch.wav", np.concatenate(parts))

# Déclencheur photo : deux claquements mécaniques rapprochés.
def clack(dur, decay, color):
    tt = t(dur)
    nz = np.convolve(rng.standard_normal(len(tt)), np.ones(color) / color, mode="same")
    return nz * np.exp(-tt * decay)
save("photo.wav", np.concatenate([clack(0.05, 120, 2), np.zeros(int(SR * 0.06)), clack(0.08, 80, 4) * 0.8]))

# Faux : buzzer de mauvaise réponse (deux tons graves légèrement désaccordés).
tt = t(0.65)
buzz = np.sign(np.sin(2 * np.pi * 150 * tt)) + np.sign(np.sin(2 * np.pi * 156 * tt))
env = np.minimum(1, tt / 0.01) * np.minimum(1, (0.65 - tt) / 0.05)
save("faux.wav", np.convolve(buzz, np.ones(6) / 6, mode="same") * env)

# Juste : carillon de bonne réponse (deux notes montantes).
def note(f, dur):
    tt = t(dur)
    return (np.sin(2 * np.pi * f * tt) + 0.3 * np.sin(2 * np.pi * 2 * f * tt)) * np.exp(-tt * 5) * np.minimum(1, tt / 0.003)
first, second = note(1047, 0.7), note(1568, 0.9)
juste = np.zeros(int(SR * 1.0))
juste[:len(first)] += first
juste[int(SR * 0.11):int(SR * 0.11) + len(second)] += second[:len(juste) - int(SR * 0.11)]
save("juste.wav", juste)

print("ok", sorted(os.listdir(OUT)))
