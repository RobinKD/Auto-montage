//! Visages du rush et cadrage des zooms (« tout le monde dans l'image »).
//!
//! Sortie : src/data/face.json -> [{t, x, y, r, s, faces}] toutes les 0,5 s, en fractions de l'image
//! (version de travail, au format du rush) :
//!   x, y   point de cadrage : centre de la zone à garder (tous les visages retenus) ; les zooms
//!          sont centrés dessus (transform-origin) ;
//!   s      zoom maximal pour que cette zone reste dans l'image (1 = pas de zoom possible, 3 au plus) ;
//!          le rendu et l'aperçu de la page des moments plafonnent tous les zooms à s ;
//!   r      demi-largeur du plus grand visage ;
//!   faces  [[personne, x, y, w, h(, x0, y0, x1, y1)], …] : chaque visage (centre, largeur, hauteur) ;
//!          « personne » : numéro de la personne (1 = la plus présente), reconnue à son visage (SFace)
//!          d'un plan à l'autre, avec sa vignette dans work/visages/p<personne>.jpg (cadrage choisi par
//!          moment sur la page des moments) ; x0…y1 : cadre de la personne quand l'image en montre
//!          plusieurs (écran partagé, incrustation, cadres sur un fond), dont le plan rapproché ne
//!          sort pas.
//! Détecteur : YuNet (work/models/face_detection_yunet_2023mar.onnx, téléchargé par la préparation,
//! avec SFace pour reconnaître les personnes), qui trouve aussi les petits visages. Visages retenus
//! pour le cadrage : ceux d'au moins 30 % de la taille du plus grand (pas le public ni les gens au
//! fond). Les trous (visage caché par une main, etc.) sont comblés par interpolation.
//!
//! Sans OpenCV : les images sont lues par ffmpeg (images brutes BGR sur un tube, toutes les 0,5 s),
//! les deux réseaux (YuNet, SFace) tournent avec tract (moteur ONNX
//! écrit en Rust, sans bibliothèque du système), et ce que faisaient cv2.FaceDetectorYN
//! (remplissage aux multiples de 32, décodage des sorties cls/obj/bbox/kps, NMS) et
//! cv2.FaceRecognizerSF (alignCrop : similitude des 5 repères vers le modèle 112x112 d'ArcFace,
//! warpAffine bilinéaire) est refait ici, ainsi que cv2.resize (bilinéaire et INTER_AREA) et
//! cv2.Sobel, aux mêmes arrondis. Sans le modèle YuNet, aucun visage n'est détecté et les zooms
//! sont centrés sur l'image.
//!
//! Reprise : les détections sont enregistrées au fil de l'eau (work/reprise/visage.json) ; une
//! préparation mise en pause ou coupée (AM_REPRISE=1) repart de la dernière ; elles restent là
//! une fois toutes faites, jusqu'à la fin de la préparation (effacées avec work/reprise).
//!
//! Cadres (panel) : quand plusieurs visages sont dans l'image, le cadre de chacun est le plus grand
//! rectangle autour de lui dont les quatre côtés sont des bords droits présents à au moins 80 % de
//! leur longueur pendant ±4 s (ou les bords de l'image), qui ne contient pas d'autre visage, et
//! qu'aucune autre ligne droite ne traverse (ce serait un autre bord). Une image filmée (deux
//! personnes sur un plateau) n'en a pas : aucun rectangle de ce genre autour des visages.

use crate::paths;
use crate::progress::{report, report_every, say};
use crate::pyjson::float_repr;
use crate::util::{self, num, round_to};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Instant;
use tract_onnx::prelude::*;

/// Similarité (cosinus) SFace au-dessus de laquelle deux visages sont la même personne.
const SAME_PERSON: f64 = 0.363;
const STEP: f64 = 0.5;
/// Image réduite pour YuNet : grand côté de 640 px.
const DETECT_LONG: f64 = 640.0;
/// Visage plus petit que 2,5 % du petit côté de l'image : ignoré.
const MIN_FACE: f64 = 0.025;
const MIN_SCORE: f32 = 0.7;
/// Seuil de recouvrement du NMS de cv2.FaceDetectorYN (valeur par défaut de create()).
const NMS_THRESHOLD: f32 = 0.3;
/// Nombre de visages gardés avant le NMS (valeur par défaut de create()).
const TOP_K: usize = 5000;
/// Visages retenus pour le cadrage : au moins 30 % de la largeur du plus grand.
const KEEP_RATIO: f64 = 0.3;
/// Un visage perdu plus longtemps commence une nouvelle piste.
const MAX_GAP: f64 = 2.0;
/// Visage vu une seule fois : fausse détection probable, ignoré.
const MIN_SEEN: usize = 2;
const LINK: f64 = 2.5;
const MAX_SCALE: f64 = 3.0;
/// Aucun visage : zooms centrés sur le haut du milieu de l'image.
const DEFAULT: (f64, f64, f64) = (0.5, 0.4, 0.2);
/// Images réduites pour chercher les cadres : grand côté de 320 px.
const PANEL_LONG: f64 = 320.0;
/// Contraste d'un bord (dérivée de Sobel, canal le plus contrasté).
const PANEL_EDGE: f32 = 20.0;
/// Côté d'un cadre : bord présent sur 80 % de sa longueur.
const PANEL_LINE: f64 = 0.8;
/// Ligne qui traverse un cadre (75 % de sa largeur ou hauteur) : pas un seul cadre.
const PANEL_CROSS: f64 = 0.75;
/// Instants voisins (±4 s) : un bord de cadre est fixe, pas ceux de l'image filmée.
const PANEL_R: usize = 8;

fn src() -> PathBuf {
    paths::rushes().join("rush_1080.mp4")
}
fn out_path() -> PathBuf {
    paths::data().join("face.json")
}
fn models() -> PathBuf {
    paths::work().join("models")
}
fn yunet_path() -> PathBuf {
    models().join("face_detection_yunet_2023mar.onnx")
}
fn sface_path() -> PathBuf {
    models().join("face_recognition_sface_2021dec.onnx")
}
fn thumbs_dir() -> PathBuf {
    paths::work().join("visages")
}
fn checkpoint_path() -> PathBuf {
    paths::work().join("reprise").join("visage.json")
}

// ---------------------------------------------------------------------------------------------
// Images (BGR, 8 bits, comme les images d'OpenCV)

/// Image BGR entrelacée, 3 octets par pixel.
#[derive(Clone)]
struct Img {
    w: usize,
    h: usize,
    data: Vec<u8>,
}

impl Img {
    fn px(&self, x: usize, y: usize, c: usize) -> u8 {
        self.data[(y * self.w + x) * 3 + c]
    }
}

/// `cv2.resize(img, (dw, dh))` (INTER_LINEAR, 8 bits) : coefficients en virgule fixe sur 11 bits
/// (INTER_RESIZE_COEF_BITS), passe horizontale en entiers, passe verticale comme la version
/// vectorisée d'OpenCV (VResizeLinearVec_32s8u).
fn resize_linear(src: &Img, dw: usize, dh: usize) -> Img {
    const SCALE: f32 = 2048.0;
    let tab = |ssize: usize, dsize: usize| -> Vec<(usize, i32, i32)> {
        let scale = ssize as f64 / dsize as f64;
        (0..dsize)
            .map(|d| {
                let f = ((d as f64 + 0.5) * scale - 0.5) as f32;
                let mut s = f.floor() as i64;
                let mut fx = f - s as f32;
                if s < 0 {
                    fx = 0.0;
                    s = 0;
                }
                if s >= ssize as i64 - 1 {
                    fx = 0.0;
                    s = ssize as i64 - 1;
                }
                let a1 = (fx * SCALE).round() as i32;
                let a0 = ((1.0 - fx) * SCALE).round() as i32;
                (s as usize, a0, a1)
            })
            .collect()
    };
    let xt = tab(src.w, dw);
    let yt = tab(src.h, dh);
    // Passe horizontale de chaque ligne source utile (valeurs × 2048).
    let hrow = |y: usize| -> Vec<i32> {
        let row = &src.data[y * src.w * 3..(y + 1) * src.w * 3];
        let mut out = vec![0i32; dw * 3];
        for (d, &(sx, a0, a1)) in xt.iter().enumerate() {
            let sx1 = (sx + 1).min(src.w - 1);
            for c in 0..3 {
                let v0 = row[sx * 3 + c] as i32;
                let v1 = if a1 != 0 { row[sx1 * 3 + c] as i32 } else { 0 };
                out[d * 3 + c] = v0 * a0 + v1 * a1;
            }
        }
        out
    };
    let mut cache: HashMap<usize, Vec<i32>> = HashMap::new();
    let mut data = vec![0u8; dw * dh * 3];
    for (dy, &(sy, b0, b1)) in yt.iter().enumerate() {
        let sy1 = (sy + 1).min(src.h - 1);
        if !cache.contains_key(&sy) {
            cache.insert(sy, hrow(sy));
        }
        if !cache.contains_key(&sy1) {
            cache.insert(sy1, hrow(sy1));
        }
        let r0 = &cache[&sy];
        let r1 = &cache[&sy1];
        let out = &mut data[dy * dw * 3..(dy + 1) * dw * 3];
        for i in 0..dw * 3 {
            let v = (((r0[i] >> 4) * b0) >> 16) + (((r1[i] >> 4) * b1) >> 16) + 2;
            out[i] = (v >> 2).clamp(0, 255) as u8;
        }
        cache.retain(|k, _| *k >= sy);
    }
    Img { w: dw, h: dh, data }
}

/// Poids de cv2.resize INTER_AREA sur un axe (computeResizeAreaTab) : [(source, destination, poids)].
fn area_tab(ssize: usize, dsize: usize) -> Vec<(usize, usize, f64)> {
    let scale = ssize as f64 / dsize as f64;
    let mut tab = vec![];
    for dx in 0..dsize {
        let fsx1 = dx as f64 * scale;
        let fsx2 = fsx1 + scale;
        let cell = scale.min(ssize as f64 - fsx1);
        let sx1 = fsx1.ceil() as i64;
        let sx2 = (fsx2.floor() as i64).min(ssize as i64);
        if sx1 as f64 - fsx1 > 1e-3 {
            tab.push(((sx1 - 1) as usize, dx, (sx1 as f64 - fsx1) / cell));
        }
        for sx in sx1..sx2 {
            tab.push((sx as usize, dx, 1.0 / cell));
        }
        if fsx2 - sx2 as f64 > 1e-3 && (sx2 as usize) < ssize {
            tab.push((sx2 as usize, dx, (fsx2 - sx2 as f64).min(1.0).min(cell) / cell));
        }
    }
    tab
}

/// Arrondi de saturate_cast<uchar> (cvRound : moitié au pair).
fn cv_round_u8(v: f64) -> u8 {
    let r = v.round_ties_even();
    r.clamp(0.0, 255.0) as u8
}

/// `cv2.resize(img, (dw, dh), interpolation=cv2.INTER_AREA)` pour une réduction : moyenne des
/// pixels couverts (facteur entier : moyenne exacte de chaque bloc).
fn resize_area(src: &Img, dw: usize, dh: usize) -> Img {
    let (sx, sy) = (src.w as f64 / dw as f64, src.h as f64 / dh as f64);
    let mut data = vec![0u8; dw * dh * 3];
    if sx.fract() == 0.0 && sy.fract() == 0.0 && sx >= 1.0 && sy >= 1.0 {
        // resizeAreaFast : somme entière × (1 / aire) en flottant, arrondie.
        let (ix, iy) = (sx as usize, sy as usize);
        let scale = 1.0f32 / (ix * iy) as f32;
        for dy in 0..dh {
            for dx in 0..dw {
                for c in 0..3 {
                    let mut sum = 0i32;
                    for y in dy * iy..(dy + 1) * iy {
                        for x in dx * ix..(dx + 1) * ix {
                            sum += src.px(x, y, c) as i32;
                        }
                    }
                    data[(dy * dw + dx) * 3 + c] = cv_round_u8((sum as f32 * scale) as f64);
                }
            }
        }
        return Img { w: dw, h: dh, data };
    }
    let xt = area_tab(src.w, dw);
    let yt = area_tab(src.h, dh);
    // Passe horizontale de chaque ligne, puis somme pondérée des lignes.
    let mut rows = vec![0f64; dh * dw * 3];
    let mut hbuf = vec![0f64; dw * 3];
    let mut by_src: Vec<Vec<(usize, f64)>> = vec![vec![]; src.h];
    for &(s, d, w) in &yt {
        by_src[s].push((d, w));
    }
    for (y, targets) in by_src.iter().enumerate() {
        if targets.is_empty() {
            continue;
        }
        hbuf.iter_mut().for_each(|v| *v = 0.0);
        for &(s, d, w) in &xt {
            for c in 0..3 {
                hbuf[d * 3 + c] += src.px(s, y, c) as f64 * w;
            }
        }
        for &(d, w) in targets {
            for i in 0..dw * 3 {
                rows[d * dw * 3 + i] += hbuf[i] * w;
            }
        }
    }
    for (o, v) in data.iter_mut().zip(rows) {
        *o = cv_round_u8(v);
    }
    Img { w: dw, h: dh, data }
}

/// Bords d'une image réduite : verticaux (v) et horizontaux (h), un octet (0/1) par pixel.
struct Edges {
    w: usize,
    h: usize,
    v: Vec<u8>,
    hz: Vec<u8>,
}

/// Bords verticaux et horizontaux nets d'une image réduite (maximum local de la dérivée).
/// cv2.Sobel(ksize=3) sur chaque canal (bord réfléchi BORDER_REFLECT_101), valeur absolue,
/// canal le plus contrasté ; voisins pris en boucle comme np.roll.
fn edge_maps(frame: &Img, size: (usize, usize)) -> Edges {
    let g = resize_area(frame, size.0, size.1);
    let (w, h) = (g.w, g.h);
    let refl = |i: i64, n: usize| -> usize {
        let n = n as i64;
        if n == 1 {
            return 0;
        }
        let mut i = i;
        if i < 0 {
            i = -i;
        }
        if i >= n {
            i = 2 * n - 2 - i;
        }
        i as usize
    };
    let mut gx = vec![0f32; w * h];
    let mut gy = vec![0f32; w * h];
    for y in 0..h {
        let ym = refl(y as i64 - 1, h);
        let yp = refl(y as i64 + 1, h);
        for x in 0..w {
            let xm = refl(x as i64 - 1, w);
            let xp = refl(x as i64 + 1, w);
            let (mut mx, mut my) = (0f32, 0f32);
            for c in 0..3 {
                let p = |xx: usize, yy: usize| g.px(xx, yy, c) as f32;
                let dx = (p(xp, ym) - p(xm, ym)) + 2.0 * (p(xp, y) - p(xm, y)) + (p(xp, yp) - p(xm, yp));
                let dy = (p(xm, yp) - p(xm, ym)) + 2.0 * (p(x, yp) - p(x, ym)) + (p(xp, yp) - p(xp, ym));
                mx = mx.max(dx.abs());
                my = my.max(dy.abs());
            }
            gx[y * w + x] = mx;
            gy[y * w + x] = my;
        }
    }
    let mut v = vec![0u8; w * h];
    let mut hz = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let a = gx[i];
            let (l, r) = (gx[y * w + (x + w - 1) % w], gx[y * w + (x + 1) % w]);
            v[i] = (a > PANEL_EDGE && a >= l && a >= r) as u8;
            let b = gy[i];
            let (u, d) = (gy[((y + h - 1) % h) * w + x], gy[((y + 1) % h) * w + x]);
            hz[i] = (b > PANEL_EDGE && b >= u && b >= d) as u8;
        }
    }
    Edges { w, h, v, hz }
}

// ---------------------------------------------------------------------------------------------
// Lecture de la vidéo par ffmpeg

/// Images par seconde, nombre d'images, largeur, hauteur, comme les donnait cv2.VideoCapture
/// (avg_frame_rate ; nb_frames du flux, sinon durée × fps arrondie).
fn probe(path: &Path) -> (f64, i64, f64, f64) {
    let ratio = |s: &str| -> f64 {
        let mut it = s.split('/');
        let a: f64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0.0);
        let b: f64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(1.0);
        if b != 0.0 {
            a / b
        } else {
            0.0
        }
    };
    let (code, out, _) = crate::procs::run(
        paths::ffprobe(),
        [
            "-v".as_ref(),
            "error".as_ref(),
            "-select_streams".as_ref(),
            "v:0".as_ref(),
            "-show_entries".as_ref(),
            "stream=width,height,avg_frame_rate,r_frame_rate,nb_frames,duration:format=duration".as_ref(),
            "-of".as_ref(),
            "json".as_ref(),
            path.as_os_str(),
        ],
    );
    if code == 0 {
        if let Ok(v) = serde_json::from_str::<Value>(&out) {
            let st = v.get("streams").and_then(|s| s.get(0)).cloned().unwrap_or(Value::Null);
            let w = st.get("width").and_then(Value::as_f64).unwrap_or(0.0);
            let h = st.get("height").and_then(Value::as_f64).unwrap_or(0.0);
            let mut fps = ratio(util::s(&st, "avg_frame_rate").unwrap_or("0/1"));
            if fps <= 0.0 {
                fps = ratio(util::s(&st, "r_frame_rate").unwrap_or("0/1"));
            }
            let pf = |x: Option<&Value>| x.and_then(Value::as_str).and_then(|s| s.parse::<f64>().ok());
            let nb = pf(st.get("nb_frames")).unwrap_or(0.0) as i64;
            let n = if nb > 0 {
                nb
            } else {
                let dur = pf(v.get("format").and_then(|f| f.get("duration"))).or(pf(st.get("duration"))).unwrap_or(0.0);
                (dur * fps + 0.5).floor() as i64
            };
            if w > 0.0 {
                return (fps, n, w, h);
            }
        }
    }
    // Sans ffprobe : ce qu'en dit « ffmpeg -i ».
    let info = crate::procs::ffmpeg_info(path);
    let re = regex::Regex::new(r"Video:.*?(\d{2,5})x(\d{2,5}).*?([\d.]+) (?:fps|tbr)").unwrap();
    let Some(c) = re.captures(&info) else { return (0.0, 0, 0.0, 0.0) };
    let fps: f64 = c[3].parse().unwrap_or(0.0);
    let dur = crate::progress::duration_of(&info);
    (fps, (dur * fps + 0.5).floor() as i64, c[1].parse().unwrap_or(0.0), c[2].parse().unwrap_or(0.0))
}

/// ffmpeg qui sort, en images brutes BGR, l'image n° int(k·0,5·fps) pour k = first, first + 1, …
/// Le filtre select garde un
/// compteur (variable 1) : une image est prise quand son numéro atteint celui du prochain instant.
struct Frames {
    child: Child,
    w: usize,
    h: usize,
}

impl Frames {
    fn open(path: &Path, fps: f64, first: usize, w: usize, h: usize) -> Option<Frames> {
        let half = fps * STEP;
        let select = format!("select=if(gte(n\\,trunc((ld(1)+{first})*{half:?}))\\,st(1\\,ld(1)+1)\\,0)");
        let child = Command::new(paths::ffmpeg())
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(path)
            .args(["-an", "-sn", "-dn", "-vf", &select, "-vsync", "0", "-f", "rawvideo", "-pix_fmt", "bgr24", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .ok()?;
        Some(Frames { child, w, h })
    }

    fn next(&mut self) -> Option<Img> {
        let mut data = vec![0u8; self.w * self.h * 3];
        self.child.stdout.as_mut()?.read_exact(&mut data).ok()?;
        Some(Img { w: self.w, h: self.h, data })
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Une image de la vidéo (n° idx), lue seule (vignettes).
fn frame_at(path: &Path, fps: f64, idx: i64, w: usize, h: usize) -> Option<Img> {
    let at = ((idx as f64 - 0.5) / fps).max(0.0);
    let out = Command::new(paths::ffmpeg())
        .args(["-v", "error", "-nostdin", "-ss", &format!("{at:.6}"), "-i"])
        .arg(path)
        .args(["-an", "-sn", "-dn", "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "bgr24", "pipe:1"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    (out.stdout.len() >= w * h * 3).then(|| Img { w, h, data: out.stdout[..w * h * 3].to_vec() })
}

// ---------------------------------------------------------------------------------------------
// Réseaux : YuNet (détection) et SFace (reconnaissance), comme cv2.FaceDetectorYN et
// cv2.FaceRecognizerSF.

type Plan = Arc<TypedRunnableModel>;

/// Charge un réseau ONNX pour une entrée de forme donnée (outputs : sorties voulues, dans l'ordre).
fn load_model(path: &Path, shape: &[usize], outputs: &[&str]) -> TractResult<Plan> {
    // Formes intermédiaires et de sortie enregistrées dans le modèle (YuNet : 640x640) ignorées :
    // elles sont recalculées pour la taille d'entrée donnée, comme le fait OpenCV.
    let mut model = tract_onnx::onnx().with_ignore_output_shapes(true).with_ignore_value_info(true).model_for_path(path)?.with_input_fact(0, f32::fact(shape).into())?;
    if !outputs.is_empty() {
        model = model.with_outputs_by_name(outputs)?;
    }
    model.into_optimized()?.into_runnable()
}

/// Visage trouvé par YuNet dans l'image réduite : boîte (x1, y1, w, h), 5 repères, score.
#[derive(Clone, Debug)]
struct Raw {
    bx: [f32; 4],
    kps: [f32; 10],
    score: f32,
}

/// Visage d'un instant, en fractions de l'image : centre, largeur, hauteur, empreinte SFace
/// normalisée (qui est qui ; None sans le modèle de reconnaissance), cadre (x0, y0, x1, y1).
#[derive(Clone, Debug)]
struct Face {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    emb: Option<Vec<f32>>,
    panel: Option<[f64; 4]>,
}

struct Detector {
    yunet: Plan,
    sface: Option<Plan>,
    size: (usize, usize),
    pad: (usize, usize),
}

const STRIDES: [usize; 3] = [8, 16, 32];

impl Detector {
    fn new(w: f64, h: f64) -> Result<Detector, String> {
        let k = DETECT_LONG / w.max(h);
        let size = (((w * k) as usize).max(1), ((h * k) as usize).max(1));
        // FaceDetectorYN : entrée remplie de noir jusqu'aux multiples de 32.
        let pad = (((size.0 - 1) / 32 + 1) * 32, ((size.1 - 1) / 32 + 1) * 32);
        let mut names = vec![];
        for kind in ["cls", "obj", "bbox", "kps"] {
            for s in STRIDES {
                names.push(format!("{kind}_{s}"));
            }
        }
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let yunet = load_model(&yunet_path(), &[1, 3, pad.1, pad.0], &names).map_err(|e| format!("{e:#}"))?;
        let sface = if sface_path().is_file() {
            match load_model(&sface_path(), &[1, 3, 112, 112], &[]) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("Modèle SFace illisible ({e:#}) : personnes non reconnues.");
                    None
                }
            }
        } else {
            None
        };
        Ok(Detector { yunet, sface, size, pad })
    }

    /// cv2.FaceDetectorYN.detect : réseau, décodage des sorties, NMS.
    fn yunet(&self, small: &Img) -> Vec<Raw> {
        let (pw, ph) = self.pad;
        let mut input = vec![0f32; 3 * pw * ph];
        for y in 0..small.h {
            for x in 0..small.w {
                for c in 0..3 {
                    input[c * pw * ph + y * pw + x] = small.px(x, y, c) as f32;
                }
            }
        }
        let Ok(tensor) = tract_ndarray::Array4::from_shape_vec((1, 3, ph, pw), input) else { return vec![] };
        let outs = match self.yunet.run(tvec!(Tensor::from(tensor).into())) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("YuNet : {e:#}");
                return vec![];
            }
        };
        let view = |i: usize| -> Vec<f32> { outs[i].try_as_plain_ram().and_then(|v| v.as_slice::<f32>().map(|s| s.to_vec())).unwrap_or_default() };
        let mut faces: Vec<Raw> = vec![];
        for (i, &stride) in STRIDES.iter().enumerate() {
            let cols = pw / stride;
            let rows = ph / stride;
            let (cls, obj, bbox, kps) = (view(i), view(i + 3), view(i + 6), view(i + 9));
            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    if idx >= cls.len() || idx >= obj.len() || idx * 4 + 3 >= bbox.len() || idx * 10 + 9 >= kps.len() {
                        continue;
                    }
                    let cs = cls[idx].clamp(0.0, 1.0);
                    let os = obj[idx].clamp(0.0, 1.0);
                    let score = (cs * os).sqrt();
                    if score < MIN_SCORE {
                        continue;
                    }
                    let s = stride as f32;
                    let cx = (c as f32 + bbox[idx * 4]) * s;
                    let cy = (r as f32 + bbox[idx * 4 + 1]) * s;
                    let w = bbox[idx * 4 + 2].exp() * s;
                    let h = bbox[idx * 4 + 3].exp() * s;
                    let mut k = [0f32; 10];
                    for n in 0..5 {
                        k[2 * n] = (kps[idx * 10 + 2 * n] + c as f32) * s;
                        k[2 * n + 1] = (kps[idx * 10 + 2 * n + 1] + r as f32) * s;
                    }
                    faces.push(Raw { bx: [cx - w / 2.0, cy - h / 2.0, w, h], kps: k, score });
                }
            }
        }
        if faces.len() <= 1 {
            return faces;
        }
        // cv::dnn::NMSBoxes sur les boîtes en entiers (Rect2i), scores > seuil, tri stable.
        let rects: Vec<[i64; 4]> = faces.iter().map(|f| [f.bx[0] as i64, f.bx[1] as i64, f.bx[2] as i64, f.bx[3] as i64]).collect();
        let mut order: Vec<usize> = (0..faces.len()).filter(|&i| faces[i].score > MIN_SCORE).collect();
        order.sort_by(|&a, &b| faces[b].score.partial_cmp(&faces[a].score).unwrap_or(std::cmp::Ordering::Equal));
        order.truncate(TOP_K);
        let overlap = |a: &[i64; 4], b: &[i64; 4]| -> f32 {
            let (aa, ab) = (a[2] * a[3], b[2] * b[3]);
            if aa + ab <= 0 {
                return 1.0 - 0.0;
            }
            let x0 = a[0].max(b[0]);
            let y0 = a[1].max(b[1]);
            let x1 = (a[0] + a[2]).min(b[0] + b[2]);
            let y1 = (a[1] + a[3]).min(b[1] + b[3]);
            let inter = if x1 > x0 && y1 > y0 { ((x1 - x0) * (y1 - y0)) as f64 } else { 0.0 };
            let dist = 1.0 - inter / (aa as f64 + ab as f64 - inter);
            1.0 - dist as f32
        };
        let mut keep: Vec<usize> = vec![];
        for i in order {
            if keep.iter().all(|&k| overlap(&rects[i], &rects[k]) <= NMS_THRESHOLD) {
                keep.push(i);
            }
        }
        keep.into_iter().map(|i| faces[i].clone()).collect()
    }

    /// Empreinte SFace d'un visage : alignCrop (112x112) puis feature, normalisée.
    fn feature(&self, plan: &Plan, small: &Img, f: &Raw) -> Option<Vec<f32>> {
        let crop = align_crop(small, &f.kps);
        let mut input = vec![0f32; 3 * 112 * 112];
        for y in 0..112 {
            for x in 0..112 {
                for c in 0..3 {
                    // blobFromImage(swapRB=True) : canaux RVB.
                    input[c * 112 * 112 + y * 112 + x] = crop.px(x, y, 2 - c) as f32;
                }
            }
        }
        let tensor = tract_ndarray::Array4::from_shape_vec((1, 3, 112, 112), input).ok()?;
        let out = plan.run(tvec!(Tensor::from(tensor).into())).ok()?;
        let mut emb: Vec<f32> = out[0].try_as_plain_ram().ok()?.as_slice::<f32>().ok()?.to_vec();
        let norm = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
        let norm = if norm != 0.0 { norm } else { 1.0 };
        emb.iter_mut().for_each(|v| *v /= norm);
        Some(emb)
    }

    /// Visages d'une image : [Face] en fractions de l'image.
    fn run(&self, frame: &Img) -> Vec<Face> {
        let small = resize_linear(frame, self.size.0, self.size.1);
        let short = self.size.0.min(self.size.1) as f64;
        let (sw, sh) = (self.size.0 as f64, self.size.1 as f64);
        let mut out = vec![];
        for f in self.yunet(&small) {
            let [fx, fy, fw, fh] = f.bx.map(|v| v as f64);
            if fw.min(fh) >= MIN_FACE * short {
                let emb = self.sface.as_ref().and_then(|p| self.feature(p, &small, &f));
                out.push(Face { x: (fx + fw / 2.0) / sw, y: (fy + fh / 2.0) / sh, w: fw / sw, h: fh / sh, emb, panel: None });
            }
        }
        out
    }
}

/// Matrice de similitude (2x3) qui porte les 5 repères du visage sur ceux du modèle 112x112
/// (getSimilarityTransformMatrix de FaceRecognizerSF : méthode d'Umeyama). Pour une matrice 2x2,
/// la rotation optimale se calcule directement : angle de (A00 + A11, A10 − A01), ou réflexion
/// quand det(A) < 0, et la somme des valeurs singulières (signées) est la norme correspondante.
fn similarity(src: &[f32; 10]) -> [f64; 6] {
    const DST: [[f32; 2]; 5] = [[38.2946, 51.6963], [73.5318, 51.5014], [56.0252, 71.7366], [41.5493, 92.3655], [70.7299, 92.2041]];
    const DST_MEAN: [f32; 2] = [56.0262, 71.9008];
    let mut mean = [0f32; 2];
    for i in 0..2 {
        mean[i] = (src[i] + src[2 + i] + src[4 + i] + src[6 + i] + src[8 + i]) / 5.0;
    }
    let mut sd = [[0f32; 2]; 5];
    let mut dd = [[0f32; 2]; 5];
    for j in 0..5 {
        for i in 0..2 {
            sd[j][i] = src[2 * j + i] - mean[i];
            dd[j][i] = DST[j][i] - DST_MEAN[i];
        }
    }
    let cov = |a: usize, b: usize| -> f64 { (0..5).map(|j| (dd[j][a] * sd[j][b]) as f64).sum::<f64>() / 5.0 };
    let (a00, a01, a10, a11) = (cov(0, 0), cov(0, 1), cov(1, 0), cov(1, 1));
    let det = a00 * a11 - a01 * a10;
    // R = U·D·Vt, trace(S·D)
    let (r, trace) = if det >= 0.0 {
        let (p, q) = (a00 + a11, a10 - a01);
        let n = p.hypot(q);
        let (c, s) = if n > 0.0 { (p / n, q / n) } else { (1.0, 0.0) };
        ([[c, -s], [s, c]], n)
    } else {
        let (p, q) = (a00 - a11, a01 + a10);
        let n = p.hypot(q);
        let (c, s) = if n > 0.0 { (p / n, q / n) } else { (1.0, 0.0) };
        ([[c, s], [s, -c]], n)
    };
    let var: f64 = (0..5).map(|j| (sd[j][0] * sd[j][0]) as f64).sum::<f64>() / 5.0 + (0..5).map(|j| (sd[j][1] * sd[j][1]) as f64).sum::<f64>() / 5.0;
    let scale = if var > 0.0 { trace / var } else { 1.0 };
    let (mx, my) = (mean[0] as f64, mean[1] as f64);
    let ts0 = r[0][0] * mx + r[0][1] * my;
    let ts1 = r[1][0] * mx + r[1][1] * my;
    [
        r[0][0] * scale,
        r[0][1] * scale,
        DST_MEAN[0] as f64 - scale * ts0,
        r[1][0] * scale,
        r[1][1] * scale,
        DST_MEAN[1] as f64 - scale * ts1,
    ]
}

/// alignCrop : cv2.warpAffine(img, M, (112, 112)) bilinéaire, fond noir, en virgule fixe comme
/// OpenCV (coordonnées au 1/32 de pixel, poids sur 15 bits).
fn align_crop(src: &Img, kps: &[f32; 10]) -> Img {
    let m = similarity(kps);
    // Matrice inverse (warpAffine sans WARP_INVERSE_MAP).
    let d = m[0] * m[4] - m[1] * m[3];
    let d = if d != 0.0 { 1.0 / d } else { 0.0 };
    let (a11, a22) = (m[4] * d, m[0] * d);
    let (m1, m3) = (-m[1] * d, -m[3] * d);
    let b1 = -a11 * m[2] - m1 * m[5];
    let b2 = -m3 * m[2] - a22 * m[5];
    let inv = [a11, m1, b1, m3, a22, b2];
    const AB_SCALE: f64 = 1024.0;
    const ROUND_DELTA: i64 = 16;
    let cvr = |v: f64| v.round_ties_even() as i64;
    let mut data = vec![0u8; 112 * 112 * 3];
    for y in 0..112usize {
        let x0 = cvr((inv[1] * y as f64 + inv[2]) * AB_SCALE) + ROUND_DELTA;
        let y0 = cvr((inv[4] * y as f64 + inv[5]) * AB_SCALE) + ROUND_DELTA;
        for x in 0..112usize {
            let xx = (x0 + cvr(inv[0] * x as f64 * AB_SCALE)) >> 5;
            let yy = (y0 + cvr(inv[3] * x as f64 * AB_SCALE)) >> 5;
            let (ix, iy) = (xx >> 5, yy >> 5);
            let (fx, fy) = (xx & 31, yy & 31);
            let w = [(32 - fx) * (32 - fy) * 32, fx * (32 - fy) * 32, (32 - fx) * fy * 32, fx * fy * 32];
            for c in 0..3 {
                let p = |px: i64, py: i64| -> i64 {
                    if px < 0 || py < 0 || px >= src.w as i64 || py >= src.h as i64 {
                        0
                    } else {
                        src.px(px as usize, py as usize, c) as i64
                    }
                };
                let v = p(ix, iy) * w[0] + p(ix + 1, iy) * w[1] + p(ix, iy + 1) * w[2] + p(ix + 1, iy + 1) * w[3];
                data[(y * 112 + x) * 3 + c] = ((v + (1 << 14)) >> 15).clamp(0, 255) as u8;
            }
        }
    }
    Img { w: 112, h: 112, data }
}

// ---------------------------------------------------------------------------------------------
// Cadrage

/// Zone à garder autour d'un visage. Seul dans l'image : le visage lui-même (les zooms restent
/// ceux d'avant, centrés sur lui, sauf s'ils le coupaient). À plusieurs : le visage avec un peu
/// d'air (cheveux, menton), pour que personne ne sorte de l'image.
fn margin_box(x: f64, y: f64, w: f64, h: f64, alone: bool) -> [f64; 4] {
    if alone {
        return [x - 0.5 * w, y - 0.5 * h, x + 0.5 * w, y + 0.5 * h];
    }
    [x - 0.75 * w, y - 0.8 * h, x + 0.75 * w, y + 0.9 * h]
}

/// Zoom maximal centré sur (px, py) qui garde la zone box = (x0, y0, x1, y1) dans l'image :
/// avec l'origine du zoom en p, l'image visible va de p·(1 - 1/s) à p + (1 - p)/s.
fn max_scale(px: f64, py: f64, b: &[f64; 4]) -> f64 {
    let (x0, y0, x1, y1) = (b[0].max(0.0), b[1].max(0.0), b[2].min(1.0), b[3].min(1.0));
    let mut s = MAX_SCALE;
    for (p, a, bb) in [(px, x0, x1), (py, y0, y1)] {
        if p - a > 1e-6 {
            s = s.min(p / (p - a));
        }
        if bb - p > 1e-6 {
            s = s.min((1.0 - p) / (bb - p));
        }
    }
    s.max(1.0)
}

/// Cadre de chaque visage (x0, y0, x1, y1 en fractions de l'image) ou None. maps : bords des
/// instants voisins (edge_maps) ; faces : visages de l'instant.
fn panels(maps: &[&Edges], faces: &[Face]) -> Vec<Option<[f64; 4]>> {
    let (w, h) = (maps[0].w, maps[0].h);
    let (wi, hi) = (w as i64, h as i64);
    let n = maps.len() as f64;
    // Bords fixes (présents à plus de 75 % des instants voisins), à un pixel près.
    let fixed = |pick: fn(&Edges) -> &Vec<u8>| -> Vec<bool> {
        (0..w * h).map(|i| maps.iter().map(|m| pick(m)[i] as f64).sum::<f64>() / n > 0.75).collect()
    };
    let v0 = fixed(|m| &m.v);
    let h0 = fixed(|m| &m.hz);
    let mut vv = vec![false; w * h];
    let mut hh = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            vv[i] = v0[i] || v0[y * w + (x + w - 1) % w] || v0[y * w + (x + 1) % w];
            hh[i] = h0[i] || h0[((y + h - 1) % h) * w + x] || h0[((y + 1) % h) * w + x];
        }
    }
    // Sommes cumulées : cV[r][c] = bords verticaux de la colonne c au-dessus de la ligne r,
    // cH[r][c] = bords horizontaux de la ligne r à gauche de la colonne c.
    let mut cv = vec![0f64; (h + 1) * w];
    for r in 0..h {
        for c in 0..w {
            cv[(r + 1) * w + c] = cv[r * w + c] + vv[r * w + c] as u8 as f64;
        }
    }
    let mut ch = vec![0f64; h * (w + 1)];
    for r in 0..h {
        for c in 0..w {
            ch[r * (w + 1) + c + 1] = ch[r * (w + 1) + c] + hh[r * w + c] as u8 as f64;
        }
    }
    // Part de la colonne c couverte par un bord entre les lignes a et b.
    let vcov = |c: i64, a: i64, b: i64| -> f64 {
        if c == 0 || c == wi {
            return 1.0;
        }
        let at = |r: i64| cv[r as usize * w + c as usize];
        (at(b) - at(a)) / (b - a).max(1) as f64
    };
    let hcov = |r: i64, a: i64, b: i64| -> f64 {
        if r == 0 || r == hi {
            return 1.0;
        }
        let at = |c: i64| ch[r as usize * (w + 1) + c as usize];
        (at(b) - at(a)) / (b - a).max(1) as f64
    };
    let peaks = |lo: i64, hi_: i64, cov: &dyn Fn(i64) -> f64| -> Vec<i64> {
        (lo..hi_).filter(|&c| cov(c) >= PANEL_LINE && cov(c) >= cov(c - 1).max(cov(c + 1))).collect()
    };
    let (mx, my) = ((0.03 * w as f64) as i64, (0.03 * h as f64) as i64);
    let (wf, hf) = (w as f64, h as f64);
    let mut out = vec![];
    for (j, f) in faces.iter().enumerate() {
        let others: Vec<(f64, f64)> = faces.iter().enumerate().filter(|(i, _)| *i != j).map(|(_, o)| (o.x * wf, o.y * hf)).collect();
        let l0 = (((f.x - f.w / 2.0) * wf) as i64).max(0);
        let t0 = (((f.y - f.h / 2.0) * hf) as i64).max(0);
        let r0 = (((f.x + f.w / 2.0) * wf).ceil() as i64).min(wi);
        let b0 = (((f.y + f.h / 2.0) * hf).ceil() as i64).min(hi);
        // Côtés possibles : bords qui couvrent la hauteur (largeur) du visage, et bords de l'image.
        let mut lefts = vec![0];
        lefts.extend(peaks(2, l0 - 1, &|c| vcov(c, t0, b0)));
        let mut rights = vec![wi];
        rights.extend(peaks(r0 + 2, wi - 2, &|c| vcov(c, t0, b0)));
        let mut tops = vec![0];
        tops.extend(peaks(2, t0 - 1, &|r| hcov(r, l0, r0)));
        let mut bots = vec![hi];
        bots.extend(peaks(b0 + 2, hi - 2, &|r| hcov(r, l0, r0)));
        let mut best: Option<((i32, f64), i64, i64, i64, i64)> = None;
        for &l in &lefts {
            for &r in &rights {
                if ((r - l) as f64) < 0.15 * wf {
                    continue;
                }
                for &t in &tops {
                    if hcov(t, l, r) < PANEL_LINE {
                        continue;
                    }
                    for &b in &bots {
                        let area = ((r - l) * (b - t)) as f64 / (wf * hf);
                        if ((b - t) as f64) < 0.15 * hf || area > 0.8 {
                            continue;
                        }
                        // côtés qui ne sont pas l'image
                        let real = (l > 0) as i32 + (t > 0) as i32 + (r < wi) as i32 + (b < hi) as i32;
                        if let Some((k, ..)) = best {
                            if (real, area) <= k {
                                continue;
                            }
                        }
                        if hcov(b, l, r) < PANEL_LINE || vcov(l, t, b) < PANEL_LINE || vcov(r, t, b) < PANEL_LINE {
                            continue;
                        }
                        if others.iter().any(|&(ox, oy)| l as f64 <= ox && ox <= r as f64 && t as f64 <= oy && oy <= b as f64) {
                            continue;
                        }
                        // (hors des bandes de 3 % le long des côtés : bord double, séparateur épais)
                        if (t + my..b - my + 1).any(|y| hcov(y, l + mx, r - mx) >= PANEL_CROSS)
                            || (l + mx..r - mx + 1).any(|x| vcov(x, t + my, b - my) >= PANEL_CROSS)
                        {
                            continue;
                        }
                        best = Some(((real, area), l, t, r, b));
                    }
                }
            }
        }
        let Some((_, mut l, mut t, mut r, mut b)) = best else {
            out.push(None);
            continue;
        };
        // Resserré sur un bord tout proche à l'intérieur (cadre posé sur un fond, séparateur épais) :
        // mieux vaut perdre un liseré de l'image que montrer le bord.
        if l + mx < l0 {
            l = (l + 1..l + mx).filter(|&x| vcov(x, t, b) >= 0.5).max().unwrap_or(l);
        }
        if r - mx > r0 {
            r = (r - mx + 1..r).filter(|&x| vcov(x, t, b) >= 0.5).min().unwrap_or(r);
        }
        if t + my < t0 {
            t = (t + 1..t + my).filter(|&y| hcov(y, l, r) >= 0.5).max().unwrap_or(t);
        }
        if b - my > b0 {
            b = (b - my + 1..b).filter(|&y| hcov(y, l, r) >= 0.5).min().unwrap_or(b);
        }
        // un pixel de marge (bord flou de l'image réduite)
        let (l, t, r, b) = (l + (l > 0) as i64, t + (t > 0) as i64, r - (r < wi) as i64, b - (b < hi) as i64);
        out.push(Some([l as f64 / wf, t as f64 / hf, r as f64 / wf, b as f64 / hf]));
    }
    out
}

/// np.percentile(values, q) (interpolation linéaire, calcul de numpy).
fn percentile(values: &[f64], q: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pos = (v.len() - 1) as f64 * q / 100.0;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(v.len() - 1);
    let t = pos - lo as f64;
    let (a, b) = (v[lo], v[hi]);
    let diff = b - a;
    if t >= 0.5 {
        b - diff * (1.0 - t)
    } else {
        a + diff * t
    }
}

/// Une piste : visages d'une même personne, par instant (n° de l'instant -> visage).
type Track = BTreeMap<usize, Face>;

/// Instants : n° k -> t = k·0,5 s ; écart de temps entre deux instants.
fn dt(a: usize, b: usize) -> f64 {
    (a as f64 - b as f64).abs() * STEP
}

/// Cadre d'une piste à chaque instant de span : celui trouvé à la plupart des instants voisins
/// (±3 s), pris au plus serré des valeurs habituelles (un bord manqué une fois ne l'agrandit pas) ;
/// {t: (x0, y0, x1, y1)}.
fn track_panels(track: &Track, span: &[usize]) -> BTreeMap<usize, [f64; 4]> {
    let usual = |found: &[[f64; 4]]| -> [f64; 4] {
        let col = |k: usize| found.iter().map(|p| p[k]).collect::<Vec<f64>>();
        [percentile(&col(0), 70.0), percentile(&col(1), 70.0), percentile(&col(2), 30.0), percentile(&col(3), 30.0)]
    };
    let seen: Vec<usize> = track.keys().copied().collect();
    let every: Vec<[f64; 4]> = seen.iter().filter_map(|u| track[u].panel).collect();
    // Cadre habituel de la piste (une piste ne passe pas d'un plan à l'autre) : pour les instants
    // où le bord a été masqué (bras, ombre), si le visage reste dedans.
    let whole = (every.len() as f64 >= 0.25 * seen.len() as f64 && !every.is_empty()).then(|| usual(&every));
    let mut out = BTreeMap::new();
    for &t in span {
        // instants de t − 3 s à t + 3 s
        let near: Vec<Option<[f64; 4]>> = seen.iter().filter(|&&u| dt(u, t) <= 3.0).map(|u| track[u].panel).collect();
        let found: Vec<[f64; 4]> = near.iter().flatten().copied().collect();
        if !found.is_empty() && found.len() as f64 >= 0.4 * near.len() as f64 {
            out.insert(t, usual(&found));
        } else if let Some(wh) = whole {
            let mut best = seen[0];
            for &u in &seen {
                if dt(u, t) < dt(best, t) {
                    best = u;
                }
            }
            let f = &track[&best];
            if wh[0] <= f.x && f.x <= wh[2] && wh[1] <= f.y && f.y <= wh[3] {
                out.insert(t, wh);
            }
        }
    }
    out
}

fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() as f64
}

/// Relie les visages d'un instant à l'autre (le plus proche, à moins de LINK largeurs de visage :
/// la caméra peut bouger vite) : pistes [(id, {t: visage})].
fn link(samples: &[(usize, Vec<Face>)]) -> Vec<(usize, Track)> {
    let mut tracks: Vec<Track> = vec![];
    let mut last: Vec<(usize, Face)> = vec![];
    for (t, found) in samples {
        let free: Vec<usize> = (0..last.len()).filter(|&i| dt(*t, last[i].0) <= MAX_GAP && *t >= last[i].0).collect();
        let mut pairs: Vec<(f64, usize, usize)> = vec![];
        for (n, f) in found.iter().enumerate() {
            for &i in &free {
                pairs.push(((f.x - last[i].1.x).hypot(f.y - last[i].1.y), n, i));
            }
        }
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut used_f = vec![false; found.len()];
        let mut used_t = vec![false; tracks.len()];
        for (d, n, i) in pairs {
            if used_f[n] || used_t[i] || d > LINK * found[n].w.max(last[i].1.w) {
                continue;
            }
            if let (Some(ea), Some(eb)) = (&found[n].emb, &last[i].1.emb) {
                if dot(ea, eb) < SAME_PERSON / 2.0 {
                    continue; // proches mais pas la même personne (changement de plan)
                }
            }
            used_f[n] = true;
            used_t[i] = true;
            tracks[i].insert(*t, found[n].clone());
            last[i] = (*t, found[n].clone());
        }
        for (n, f) in found.iter().enumerate() {
            if !used_f[n] {
                tracks.push(BTreeMap::from([(*t, f.clone())]));
                last.push((*t, f.clone()));
            }
        }
    }
    let alone = tracks.len() == 1;
    tracks.into_iter().enumerate().filter(|(_, tr)| tr.len() >= MIN_SEEN || alone).collect()
}

/// Regroupe les pistes par personne (empreintes SFace moyennes proches) : {piste: personne},
/// personnes numérotées à partir de 1, de la plus présente à la moins présente. Sans empreintes,
/// une piste = une personne.
fn persons(tracks: &[(usize, Track)]) -> HashMap<usize, usize> {
    let mut order: Vec<&(usize, Track)> = tracks.iter().collect();
    order.sort_by_key(|(_, tr)| std::cmp::Reverse(tr.len()));
    // [(somme des empreintes, [pistes])]
    let mut groups: Vec<(Option<Vec<f64>>, Vec<usize>)> = vec![];
    let norm = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    for (i, tr) in order {
        let embs: Vec<&Vec<f32>> = tr.values().filter_map(|f| f.emb.as_ref()).collect();
        let mean: Option<Vec<f64>> = (!embs.is_empty()).then(|| {
            let mut m = vec![0f64; embs[0].len()];
            for e in &embs {
                for (a, b) in m.iter_mut().zip(e.iter()) {
                    *a += *b as f64;
                }
            }
            m.iter_mut().for_each(|a| *a /= embs.len() as f64);
            let nn = norm(&m);
            let nn = if nn != 0.0 { nn } else { 1.0 };
            m.iter().map(|a| a / nn).collect()
        });
        if let Some(mean) = &mean {
            let mut best: (f64, Option<usize>) = (-1.0, None);
            for (k, g) in groups.iter().enumerate() {
                if let Some(sum) = &g.0 {
                    let ng = norm(sum);
                    let sim: f64 = mean.iter().zip(sum).map(|(a, b)| a * (b / ng)).sum();
                    // max((similarité, k)) de Python : à égalité, le dernier groupe
                    if best.1.is_none() || sim >= best.0 {
                        best = (sim, Some(k));
                    }
                }
            }
            if let (true, Some(k)) = (best.0 >= SAME_PERSON, best.1) {
                let g = &mut groups[k];
                if let Some(sum) = &mut g.0 {
                    for (a, b) in sum.iter_mut().zip(mean) {
                        *a += b * embs.len() as f64;
                    }
                }
                g.1.push(*i);
                continue;
            }
        }
        groups.push((mean.map(|m| m.iter().map(|a| a * embs.len() as f64).collect()), vec![*i]));
    }
    let len_of: HashMap<usize, usize> = tracks.iter().map(|(i, tr)| (*i, tr.len())).collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.1.iter().map(|i| len_of[i]).sum::<usize>()));
    let mut who = HashMap::new();
    for (k, g) in groups.iter().enumerate() {
        for i in &g.1 {
            who.insert(*i, k + 1);
        }
    }
    who
}

/// Vignette de chaque personne (work/visages/p<n>.jpg, 160 px) : son plus grand visage.
fn thumbnails(fps: f64, w: usize, h: usize, tracks: &[(usize, Track)], who: &HashMap<usize, usize>) {
    let dir = thumbs_dir();
    util::remove_all(&dir);
    util::mkdirs(&dir);
    let mut best: BTreeMap<usize, (usize, Face)> = BTreeMap::new();
    for (i, tr) in tracks {
        for (t, f) in tr {
            let p = who[i];
            if best.get(&p).map(|b| f.w > b.1.w).unwrap_or(true) {
                best.insert(p, (*t, f.clone()));
            }
        }
    }
    for (p, (t, f)) in best {
        let Some(frame) = frame_at(&src(), fps, (t as f64 * STEP * fps) as i64, w, h) else { continue };
        let (wf, hf) = (w as f64, h as f64);
        let half = (f.w * wf).max(f.h * hf) * 0.8;
        let x0 = (f.x * wf - half).max(0.0) as usize;
        let y0 = (f.y * hf - half).max(0.0) as usize;
        let x1 = ((f.x * wf + half).min(wf) as usize).min(w);
        let y1 = ((f.y * hf + half).min(hf) as usize).min(h);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let mut crop = Img { w: x1 - x0, h: y1 - y0, data: Vec::with_capacity((x1 - x0) * (y1 - y0) * 3) };
        for y in y0..y1 {
            crop.data.extend_from_slice(&frame.data[(y * w + x0) * 3..(y * w + x1) * 3]);
        }
        let small = resize_linear(&crop, 160, 160);
        let rgb: Vec<u8> = small.data.chunks(3).flat_map(|c| [c[2], c[1], c[0]]).collect();
        if let Ok(file) = std::fs::File::create(dir.join(format!("p{p}.jpg"))) {
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 85);
            let _ = enc.encode(&rgb, 160, 160, image::ExtendedColorType::Rgb8);
        }
    }
}

/// Moyenne glissante sur n valeurs, bords prolongés (np.convolve sur np.pad(…, mode="edge")).
fn smooth(values: &[f64], n: usize) -> Vec<f64> {
    if values.len() < n {
        return values.to_vec();
    }
    let k = 1.0 / n as f64;
    let half = n / 2;
    let at = |i: i64| values[i.clamp(0, values.len() as i64 - 1) as usize];
    (0..values.len())
        .map(|i| (0..n).map(|j| at(i as i64 + j as i64 - half as i64) * k).sum::<f64>())
        .collect()
}

/// np.interp(x, xp, fp) (xp croissants).
fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    if x <= xp[0] {
        return fp[0];
    }
    if x >= xp[xp.len() - 1] {
        return fp[fp.len() - 1];
    }
    let j = xp.partition_point(|&v| v <= x) - 1;
    if xp[j] == x {
        return fp[j];
    }
    let slope = (fp[j + 1] - fp[j]) / (xp[j + 1] - xp[j]);
    slope * (x - xp[j]) + fp[j]
}

/// Positions d'une piste à chaque instant entre sa première et sa dernière détection (trous
/// de moins de MAX_GAP comblés), lissées : {t: (x, y, w, h)}.
fn track_positions(track: &Track, count: usize) -> BTreeMap<usize, [f64; 4]> {
    let seen: Vec<usize> = track.keys().copied().collect();
    let (first, last) = (seen[0], seen[seen.len() - 1]);
    let span: Vec<usize> = (0..count).filter(|&t| first <= t && t <= last).collect();
    let xp: Vec<f64> = seen.iter().map(|&u| u as f64 * STEP).collect();
    let cols: Vec<Vec<f64>> = (0..4)
        .map(|k| {
            let fp: Vec<f64> = seen.iter().map(|u| {
                let f = &track[u];
                [f.x, f.y, f.w, f.h][k]
            }).collect();
            smooth(&span.iter().map(|&t| interp(t as f64 * STEP, &xp, &fp)).collect::<Vec<f64>>(), 5)
        })
        .collect();
    let mut out = BTreeMap::new();
    for (j, &t) in span.iter().enumerate() {
        let k = seen.partition_point(|&u| u < t);
        let before = if k > 0 && seen[k] != t { seen[k - 1] } else { t };
        let after = seen[k];
        if dt(after, before) <= MAX_GAP {
            out.insert(t, [cols[0][j], cols[1][j], cols[2][j], cols[3][j]]);
        }
    }
    out
}

/// np.median.
fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

// ---------------------------------------------------------------------------------------------
// Reprise

fn face_json(f: &Face) -> Value {
    json!([
        num(f.x),
        num(f.y),
        num(f.w),
        num(f.h),
        f.emb.as_ref().map(|e| Value::Array(e.iter().map(|v| num(round_to(*v as f64, 5))).collect())).unwrap_or(Value::Null),
        f.panel.map(|p| Value::Array(p.iter().map(|v| num(*v)).collect())).unwrap_or(Value::Null)
    ])
}

fn face_from_json(v: &Value) -> Option<Face> {
    let a = v.as_array()?;
    let g = |i: usize| a.get(i).and_then(Value::as_f64);
    let emb = match a.get(4)? {
        Value::Null => None,
        Value::Array(e) => Some(e.iter().map(|x| x.as_f64().map(|x| x as f32)).collect::<Option<Vec<f32>>>()?),
        _ => return None,
    };
    let panel = match a.get(5) {
        None | Some(Value::Null) => None,
        Some(Value::Array(p)) if p.len() == 4 => Some([p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?, p[3].as_f64()?]),
        _ => return None,
    };
    Some(Face { x: g(0)?, y: g(1)?, w: g(2)?, h: g(3)?, emb, panel })
}

/// Détections enregistrées (reprise), si la clé est la même.
fn load_checkpoint(key: &str) -> Option<Vec<(usize, Vec<Face>)>> {
    let saved = util::read_json(checkpoint_path())?;
    if std::env::var("AM_REPRISE").ok().as_deref() != Some("1") || saved.get("key")?.as_str()? != key {
        return None;
    }
    let mut samples = vec![];
    for s in saved.get("samples")?.as_array()? {
        let t = s.get(0)?.as_f64()?;
        let found = s.get(1)?.as_array()?.iter().map(face_from_json).collect::<Option<Vec<Face>>>()?;
        samples.push(((t / STEP).round() as usize, found));
    }
    Some(samples)
}

/// Enregistre les done premiers instants (cadres déjà cherchés).
fn save_checkpoint(key: &str, samples: &[(usize, Vec<Face>)]) {
    let path = checkpoint_path();
    util::mkdirs(path.parent().unwrap());
    let list: Vec<Value> = samples
        .iter()
        .map(|(t, found)| json!([num(*t as f64 * STEP), Value::Array(found.iter().map(face_json).collect())]))
        .collect();
    let _ = util::write_json_atomic(&path, &json!({"key": key, "samples": list}), None);
}

// ---------------------------------------------------------------------------------------------

pub fn main(_args: &[String]) -> i32 {
    util::mkdirs(paths::data());
    let source = src();
    let (fps, n, w, h) = if source.is_file() { probe(&source) } else { (0.0, 0, 0.0, 0.0) };
    let (wu, hu) = (w as usize, h as usize);
    // YuNet (et SFace) ; sans le modèle YuNet, pas de détection (plus de cascade de Haar).
    let detector = if yunet_path().is_file() && w > 0.0 && h > 0.0 {
        match Detector::new(w, h) {
            Ok(d) => Some(d),
            Err(e) => {
                say(&format!("Modèle YuNet illisible ({e}) : zooms centrés sur l'image."));
                None
            }
        }
    } else {
        say("Détection du visage indisponible (modèle YuNet absent) : zooms centrés sur l'image.");
        None
    };
    let name = "YuNet";
    let duration = if fps > 0.0 { n as f64 / fps } else { 0.0 };
    // np.arange(0, n / fps, STEP) : instants k·0,5 s
    let mut count = if fps > 0.0 && n > 0 { (duration / STEP).ceil().max(0.0) as usize } else { 0 };
    let size = std::fs::metadata(&source).map(|m| m.len()).unwrap_or(0);
    let key = format!("{name} {n} {} {}x{} {size}", float_repr(fps), float_repr(w), float_repr(h));
    let mut samples: Vec<(usize, Vec<Face>)> = vec![];
    if let Some(saved) = load_checkpoint(&key) {
        samples = saved;
        let at = samples.last().map(|s| s.0 as f64 * STEP).unwrap_or(0.0);
        say(&format!("Reprise de la position du visage à {} s", util::round_int(at)));
    }
    let k = if w > 0.0 && h > 0.0 { PANEL_LONG / w.max(h) } else { 1.0 };
    let small = (((w * k) as usize).max(1), ((h * k) as usize).max(1));

    let resumed = samples.len();
    let mut maps: Vec<Edges> = vec![];
    // Cadres des visages de l'instant j (quand il y en a plusieurs), d'après les bords des
    // instants voisins (maps garde les PANEL_R précédents et suivants).
    let add_panels = |samples: &mut Vec<(usize, Vec<Face>)>, maps: &[Edges], j: usize| {
        let off = samples.len() - maps.len(); // instant de maps[0]
        let lo = (j as i64 - PANEL_R as i64 - off as i64).max(0) as usize;
        let hi = ((j + PANEL_R + 1) as i64 - off as i64).clamp(0, maps.len() as i64) as usize;
        let near: Vec<&Edges> = maps[lo.min(hi)..hi].iter().collect();
        let found = &mut samples[j].1;
        if found.len() > 1 && !near.is_empty() {
            let ps = panels(&near, found);
            for (f, p) in found.iter_mut().zip(ps) {
                f.panel = p;
            }
        } else {
            found.iter_mut().for_each(|f| f.panel = None);
        }
    };
    if let Some(det) = &detector {
        let mut saved_at = Instant::now();
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 8);
        if resumed < count {
            if let Some(mut frames) = Frames::open(&source, fps, resumed, wu, hu) {
                let mut next = resumed;
                'read: while next < count {
                    // Un lot d'images lues, analysées en parallèle (visages, bords), puis rangées dans l'ordre.
                    let mut batch = vec![];
                    while batch.len() < workers * 2 && next + batch.len() < count {
                        match frames.next() {
                            Some(img) => batch.push(img),
                            None => break,
                        }
                    }
                    let ended = batch.len() < workers * 2 && next + batch.len() < count;
                    let chunk = batch.len().div_ceil(workers).max(1);
                    let results: Vec<(Vec<Face>, Edges)> = std::thread::scope(|s| {
                        let handles: Vec<_> = batch
                            .chunks(chunk)
                            .map(|part| s.spawn(move || part.iter().map(|img| (det.run(img), edge_maps(img, small))).collect::<Vec<_>>()))
                            .collect();
                        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
                    });
                    for (found, edges) in results {
                        let t = next;
                        report("Position du visage", t as f64 * STEP, duration);
                        if saved_at.elapsed().as_secs_f64() > 20.0 {
                            let done = resumed.max(samples.len().saturating_sub(PANEL_R));
                            save_checkpoint(&key, &samples[..done]);
                            saved_at = Instant::now();
                        }
                        samples.push((t, found));
                        maps.push(edges);
                        if maps.len() > 2 * PANEL_R + 1 {
                            maps.remove(0);
                        }
                        if samples.len() >= 1 + PANEL_R && samples.len() - 1 - PANEL_R >= resumed {
                            let j = samples.len() - 1 - PANEL_R;
                            add_panels(&mut samples, &maps, j);
                        }
                        next += 1;
                    }
                    if ended {
                        break 'read;
                    }
                }
            }
        }
        for j in resumed.max(samples.len().saturating_sub(PANEL_R))..samples.len() {
            add_panels(&mut samples, &maps, j);
        }
        report_every("Position du visage", duration, duration, 0.0);
        // Détections gardées jusqu'à la fin de la préparation : une préparation abandonnée en
        // gardant la position du visage (page « Rushes et montages ») la reprend sans rien refaire.
        save_checkpoint(&key, &samples);
    }
    if count == 0 {
        // durée illisible : celle du son (écrit par la préparation juste avant)
        if let Ok(r) = hound::WavReader::open(paths::work().join("rush_16k.wav")) {
            let spec = r.spec();
            let secs = r.duration() as f64 / spec.sample_rate as f64;
            count = ((secs + STEP) / STEP).ceil().max(0.0) as usize;
        }
    }

    let linked = link(&samples);
    let who = persons(&linked);
    if detector.is_some() {
        thumbnails(fps, wu, hu, &linked, &who);
    }
    let tracks: Vec<(usize, BTreeMap<usize, [f64; 4]>)> = linked.iter().map(|(i, tr)| (*i, track_positions(tr, count))).collect();
    let frames_panels: Vec<(usize, BTreeMap<usize, [f64; 4]>)> = linked
        .iter()
        .zip(&tracks)
        .map(|((i, tr), (_, pos))| (*i, track_panels(tr, &pos.keys().copied().collect::<Vec<usize>>())))
        .collect();
    // (t, visages présents [(personne, x, y, w, h)], cadres {personne: (x0, y0, x1, y1)})
    type Frame = (usize, Vec<(usize, [f64; 4])>, BTreeMap<usize, [f64; 4]>);
    let mut frames: Vec<Frame> = (0..count)
        .map(|t| {
            let faces = tracks.iter().filter_map(|(i, pos)| pos.get(&t).map(|p| (who[i], *p))).collect();
            let boxes = frames_panels.iter().filter_map(|(i, p)| p.get(&t).map(|b| (who[i], *b))).collect();
            (t, faces, boxes)
        })
        .collect();
    // Cadre manqué (bord masqué un moment, piste coupée) : celui de la même personne trouvé le plus
    // près dans le temps (15 s au plus), si son visage est dedans.
    let mut known_p: BTreeMap<usize, Vec<(usize, [f64; 4])>> = BTreeMap::new();
    for (t, _, boxes) in &frames {
        for (p, b) in boxes {
            known_p.entry(*p).or_default().push((*t, *b));
        }
    }
    for (t, faces, boxes) in frames.iter_mut() {
        if faces.len() <= 1 {
            continue;
        }
        for (p, f) in faces.iter() {
            if boxes.contains_key(p) {
                continue;
            }
            let Some(list) = known_p.get(p) else { continue };
            let mut best = list[0];
            for kb in list {
                if dt(kb.0, *t) < dt(best.0, *t) {
                    best = *kb;
                }
            }
            let (u, b) = best;
            if dt(u, *t) <= 15.0 && b[0] <= f[0] && f[0] <= b[2] && b[1] <= f[1] && f[1] <= b[3] {
                boxes.insert(*p, b);
            }
        }
    }

    // Zone à garder à chaque instant : visages retenus (au moins KEEP_RATIO du plus grand).
    let mut raw: Vec<Option<([f64; 4], f64)>> = frames
        .iter()
        .map(|(_, faces, _)| {
            if faces.is_empty() {
                return None;
            }
            let big = faces.iter().map(|f| f.1[2]).fold(f64::MIN, f64::max);
            let kept: Vec<&(usize, [f64; 4])> = faces.iter().filter(|f| f.1[2] >= KEEP_RATIO * big).collect();
            let boxes: Vec<[f64; 4]> = kept.iter().map(|f| margin_box(f.1[0], f.1[1], f.1[2], f.1[3], kept.len() == 1)).collect();
            let b = [
                boxes.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min),
                boxes.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min),
                boxes.iter().map(|b| b[2]).fold(f64::NEG_INFINITY, f64::max),
                boxes.iter().map(|b| b[3]).fold(f64::NEG_INFINITY, f64::max),
            ];
            Some((b, big / 2.0))
        })
        .collect();
    let known: Vec<usize> = (0..raw.len()).filter(|&k| raw[k].is_some()).collect();
    if known.is_empty() {
        say("Aucun visage détecté : zooms centrés sur l'image.");
    }
    // Entrées : (t, x, y, r, s, zone, visages)
    struct Entry {
        t: usize,
        x: f64,
        y: f64,
        r: f64,
        s: f64,
        bx: Option<[f64; 4]>,
        faces: Vec<Value>,
    }
    let mut res: Vec<Entry> = vec![];
    for (k, (t, faces, boxes)) in frames.iter().enumerate() {
        if raw[k].is_none() && !known.is_empty() {
            // pas de visage : zone du plus proche instant avec visage
            let mut j = known[0];
            for &c in &known {
                if (c as i64 - k as i64).abs() < (j as i64 - k as i64).abs() {
                    j = c;
                }
            }
            raw[k] = raw[j];
        }
        let Some((bx, r)) = raw[k] else {
            let (x, y, r) = DEFAULT;
            res.push(Entry { t: *t, x, y, r, s: MAX_SCALE, bx: None, faces: vec![] });
            continue;
        };
        let fl: Vec<Value> = faces
            .iter()
            .map(|(p, f)| {
                let mut v = vec![json!(*p)];
                v.extend(f.iter().map(|x| num(round_to(*x, 4))));
                if let Some(b) = boxes.get(p) {
                    v.extend(b.iter().map(|x| num(round_to(*x, 4))));
                }
                Value::Array(v)
            })
            .collect();
        res.push(Entry { t: *t, x: (bx[0] + bx[2]) / 2.0, y: (bx[1] + bx[3]) / 2.0, r, s: MAX_SCALE, bx: Some(bx), faces: fl });
    }
    if !res.is_empty() {
        // Lissage du point de cadrage ; zoom maximal pris au plus serré des instants voisins
        // (le zoom se réduit avant qu'une deuxième personne entre dans l'image).
        let xs = smooth(&res.iter().map(|p| p.x).collect::<Vec<f64>>(), 5);
        let ys = smooth(&res.iter().map(|p| p.y).collect::<Vec<f64>>(), 5);
        let rs = smooth(&res.iter().map(|p| p.r).collect::<Vec<f64>>(), 5);
        for (j, p) in res.iter_mut().enumerate() {
            p.x = round_to(xs[j], 4);
            p.y = round_to(ys[j], 4);
            p.r = round_to(rs[j], 4);
        }
        let caps: Vec<f64> = res.iter().map(|p| p.bx.map(|b| max_scale(p.x, p.y, &b)).unwrap_or(p.s)).collect();
        for (j, p) in res.iter_mut().enumerate() {
            let lo = j.saturating_sub(2);
            let hi = (j + 3).min(caps.len());
            p.s = round_to(caps[lo..hi].iter().copied().fold(f64::INFINITY, f64::min), 3);
        }
    }
    // Ordre des clés : « s » après « faces » quand une zone a été calculée.
    let out: Vec<Value> = res
        .iter()
        .map(|p| {
            let mut m = Map::new();
            m.insert("t".into(), num(p.t as f64 * STEP));
            m.insert("x".into(), num(p.x));
            m.insert("y".into(), num(p.y));
            m.insert("r".into(), num(p.r));
            if p.bx.is_none() {
                m.insert("s".into(), num(p.s));
            }
            m.insert("faces".into(), Value::Array(p.faces.clone()));
            if p.bx.is_some() {
                m.insert("s".into(), num(p.s));
            }
            Value::Object(m)
        })
        .collect();
    if let Err(e) = util::write_json(out_path(), &Value::Array(out), None) {
        eprintln!("Écriture de {} impossible : {e}", out_path().display());
        return 1;
    }
    let many = frames.iter().filter(|f| f.1.len() > 1).count();
    let framed = frames.iter().filter(|f| !f.2.is_empty()).count();
    if !known.is_empty() {
        let mut people: Vec<usize> = who.values().copied().collect();
        people.sort();
        people.dedup();
        say(&format!(
            "{name} : visage trouvé à {}/{} instants, plusieurs visages à {many}, {} personne(s) ({} pistes), chacune dans son cadre à {framed}, zoom maximal médian ×{:.2}",
            known.len(),
            frames.len(),
            people.len(),
            tracks.len(),
            median(&res.iter().map(|p| p.s).collect::<Vec<f64>>())
        ));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similitude_identite() {
        // Repères déjà à la place du modèle : transformation identité.
        let kps = [38.2946, 51.6963, 73.5318, 51.5014, 56.0252, 71.7366, 41.5493, 92.3655, 70.7299, 92.2041];
        let m = similarity(&kps);
        assert!((m[0] - 1.0).abs() < 1e-3 && m[1].abs() < 1e-3 && m[3].abs() < 1e-3 && (m[4] - 1.0).abs() < 1e-3);
        assert!(m[2].abs() < 0.05 && m[5].abs() < 0.05);
        // Visage deux fois plus grand et décalé : échelle 1/2.
        let big: Vec<f32> = kps.iter().enumerate().map(|(i, v)| v * 2.0 + if i % 2 == 0 { 100.0 } else { 40.0 }).collect();
        let m = similarity(&big.try_into().unwrap());
        assert!((m[0] - 0.5).abs() < 1e-3 && (m[4] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn percentile_numpy() {
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 70.0), 3.1);
        assert_eq!(percentile(&[5.0], 30.0), 5.0);
        assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), 2.5);
    }

    #[test]
    fn lissage_et_interpolation() {
        assert_eq!(smooth(&[1.0, 2.0], 5), vec![1.0, 2.0]);
        let s = smooth(&[0.0, 0.0, 5.0, 0.0, 0.0, 0.0], 5);
        assert!((s[0] - 1.0).abs() < 1e-12 && (s[2] - 1.0).abs() < 1e-12 && s[5].abs() < 1e-12);
        assert_eq!(interp(1.5, &[1.0, 2.0], &[10.0, 20.0]), 15.0);
        assert_eq!(interp(0.0, &[1.0, 2.0], &[10.0, 20.0]), 10.0);
    }

    #[test]
    fn reduction_bilineaire_facteur_entier() {
        // Facteur 3 : OpenCV prend le pixel du milieu de chaque bloc.
        let data: Vec<u8> = (0..9 * 6 * 3).map(|i| (i % 251) as u8).collect();
        let img = Img { w: 9, h: 6, data };
        let s = resize_linear(&img, 3, 2);
        for y in 0..2 {
            for x in 0..3 {
                for c in 0..3 {
                    assert_eq!(s.px(x, y, c), img.px(3 * x + 1, 3 * y + 1, c));
                }
            }
        }
        let a = resize_area(&img, 3, 2);
        let mean: u32 = (0..3).flat_map(|y| (0..3).map(move |x| (x, y))).map(|(x, y)| img.px(x, y, 0) as u32).sum();
        assert_eq!(a.px(0, 0, 0), cv_round_u8(mean as f64 / 9.0));
    }

    #[test]
    fn zoom_maximal() {
        assert!((max_scale(0.5, 0.5, &[0.3, 0.3, 0.7, 0.7]) - 2.5).abs() < 1e-9);
        assert_eq!(max_scale(0.5, 0.5, &[0.45, 0.45, 0.55, 0.55]), MAX_SCALE);
        assert_eq!(max_scale(0.5, 0.5, &[0.0, 0.0, 1.0, 1.0]), 1.0);
    }
}
