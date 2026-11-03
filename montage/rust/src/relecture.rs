//! Relecture de la transcription (appelée par transcribe_segments, après Parakeet et Whisper) :
//! repérage au dictionnaire, puis seconde écoute par Whisper.
//!
//! 1. Dictionnaires (fréquences des mots du français et de l'anglais, tirées de sous-titres de films :
//!    paquet pyspellchecker, licence MIT, installés par setup_whisper dans `dictionnaires/` à côté de
//!    whisper.cpp). Chaque mot d'un segment dans la langue du rush est classé : connu, d'une autre langue
//!    (absent de la langue du rush, ou au moins `FOREIGN_RATIO` fois plus courant dans l'autre :
//!    « and », « the », « you »), ou inconnu (dans aucun des deux : « smartwatchs », « monts »
//!    sans être dans la langue… ; un mot inconnu à majuscule est pris pour un nom propre et laissé).
//!    Sans dictionnaire, seuls les mots de confiance basse (moments_lib::DOUBT) sont relus.
//! 2. Mots suspects : confiance basse, autre langue, ou inconnus (pas les « euh » ni les bruits).
//!    Leur segment est réécouté seul par Whisper, forcé dans la langue du rush, puis les deux textes
//!    sont comparés mot à mot (sans ponctuation, sans majuscules, sans « euh ») :
//!    - mêmes mots : le doute est levé (« v »: 0) ;
//!    - mots différents autour d'un mot suspect : si Parakeet a écrit un mot d'une autre langue
//!      (« One minute ») et que Whisper n'écrit que des mots connus de la langue du rush, la version de
//!      Whisper est prise (temps des mots répartis sur ceux de Parakeet), sinon celle de Parakeet est
//!      gardée (un mot seulement inconnu, souvent un nom ou un mot rare, « macrout ») ; dans les deux
//!      cas le mot reste à vérifier (« v »: 1) et l'autre version est proposée (« alt » : texte,
//!      « altn » : nombre de mots qu'elle remplace s'il y en a plus d'un, "" pour retirer les mots,
//!      comme « un peu de temps » inventé par Parakeet), sauf si elle ressemble à une invention de
//!      Whisper (`invented`) ;
//!    - au moins `LOST_WORDS` mots entendus par Whisper seul (Parakeet en perd parfois) : proposés
//!      sur le mot qui les précède.
//!    Les différences loin de tout mot suspect sont laissées : Parakeet est le plus juste des deux
//!    ailleurs (parakeet/comparaison-whisper-parakeet.md).
//!
//! Segments surtout dans une autre langue (`mostly_foreign`, ou transcribe_segments::foreign) en
//! îlot dans un passage dans la langue du rush (transcribe_segments::second_listen) : Whisper les
//! réécoute sans langue imposée ; entendus dans la langue du rush, Parakeet les avait traduits et le
//! texte de Whisper les remplace (à vérifier) ; sinon ils y sont vraiment et restent tels quels
//! (forcé dans la langue du rush, Whisper les traduirait).
//!
//! Champs ajoutés aux mots de work/segments.json : « v » (1 : à vérifier, 0 : vérifié par la seconde
//! écoute ; absent : d'après la confiance « p »), « alt », « altn » ; repris par make_moments
//! (« d », « alt », « altn » des mots de la page des moments).

use crate::difflib::{SequenceMatcher, Tag};
use crate::moments_lib::DOUBT;
use crate::paths;
use crate::redites::is_filler;
use crate::util::round_to;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

/// Un mot est d'une autre langue s'il y est au moins tant de fois plus courant (fréquences relatives)
/// que dans celle du rush : « and » (200), « for » (350), « show » (29) ; « stop » (18), « place » (2),
/// « information » (5) restent français.
const FOREIGN_RATIO: f64 = 20.0;
/// Mots entendus par Whisper seul, proposés en ajout à partir de ce nombre (un seul mot ajouté est le
/// plus souvent une hésitation ou une répétition que Parakeet a eu raison de laisser).
pub const LOST_WORDS: usize = 2;
/// Langues des dictionnaires (moments_lib::LANGUAGES).
pub const DICT_LANGS: [&str; 2] = ["fr", "en"];
/// Mots courants à l'oral absents des dictionnaires (ou plus courants en anglais), par langue.
const SPOKEN: [(&str, &str); 2] = [
    ("fr", "ok okay ouais ouah waouh wouah youpi bah ben bof hop pff ouf oups chut ah oh eh hé"),
    ("en", "ok okay yeah yep nope gonna wanna gotta kinda sorta ah oh hey wow"),
];
/// Élisions du français : « l'information » se cherche comme « information ».
const ELISIONS: [&str; 13] = ["l'", "d'", "j'", "m'", "n'", "s'", "t'", "c'", "qu'", "jusqu'", "lorsqu'", "puisqu'", "quoiqu'"];

/// Dossier des dictionnaires (`<langue>.json.gz`), à côté de whisper.cpp.
pub fn dict_dir() -> PathBuf {
    let w = paths::whisper_dir();
    w.parent().map(PathBuf::from).unwrap_or_else(|| paths::root().to_path_buf()).join("dictionnaires")
}

/// Fréquences relatives des mots d'une langue.
struct Freq {
    rel: HashMap<String, f64>,
}

impl Freq {
    fn load(lang: &str) -> Option<Freq> {
        let f = std::fs::File::open(dict_dir().join(format!("{lang}.json.gz"))).ok()?;
        let mut text = String::new();
        flate2::read::GzDecoder::new(f).read_to_string(&mut text).ok()?;
        let map: Map<String, Value> = serde_json::from_str(&text).ok()?;
        let total: f64 = map.values().filter_map(Value::as_f64).sum::<f64>().max(1.0);
        Some(Freq { rel: map.into_iter().filter_map(|(k, v)| Some((k, v.as_f64()? / total))).collect() })
    }
    fn get(&self, w: &str) -> f64 {
        self.rel.get(w).copied().unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Known,
    Foreign,
    Unknown,
    Name,
}

/// Dictionnaires de la langue du rush et de l'autre langue.
pub struct Dicts {
    lang: String,
    own: Freq,
    other: Freq,
}

impl Dicts {
    /// Dictionnaires installés (None si l'un manque, ou langue sans dictionnaire).
    pub fn load(lang: &str) -> Option<Dicts> {
        let other = DICT_LANGS.iter().find(|l| **l != lang)?;
        if !DICT_LANGS.contains(&lang) {
            return None;
        }
        Some(Dicts { lang: lang.to_string(), own: Freq::load(lang)?, other: Freq::load(other)? })
    }

    #[cfg(test)]
    fn from_counts(lang: &str, own: &[(&str, f64)], other: &[(&str, f64)]) -> Dicts {
        let freq = |list: &[(&str, f64)]| {
            let total: f64 = list.iter().map(|x| x.1).sum::<f64>().max(1.0);
            Freq { rel: list.iter().map(|(w, c)| (w.to_string(), c / total)).collect() }
        };
        Dicts { lang: lang.to_string(), own: freq(own), other: freq(other) }
    }

    /// Classe d'un mot nettoyé (minuscules) : fréquences dans les deux langues.
    fn class_of(&self, w: &str) -> Kind {
        let spoken = SPOKEN.iter().find(|(l, _)| *l == self.lang).map(|(_, v)| *v).unwrap_or("");
        if spoken.split(' ').any(|x| x == w) || is_filler(w) {
            return Kind::Known;
        }
        let (o, x) = (self.own.get(w), self.other.get(w));
        if o == 0.0 && x == 0.0 {
            Kind::Unknown
        } else if x >= FOREIGN_RATIO * o {
            Kind::Foreign
        } else {
            Kind::Known
        }
    }

    /// Classe d'un mot tel qu'écrit par le modèle (« L'information, », « peut-être », « It's »).
    pub fn kind(&self, word: &str) -> Kind {
        let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
        if trimmed.is_empty() || trimmed.chars().any(|c| c.is_ascii_digit()) {
            return Kind::Known;
        }
        let w = clean(trimmed);
        let whole = self.class_of(&w);
        let kind = if whole != Kind::Unknown {
            whole
        } else {
            // Élision (« jusqu'ici »), mot anglais à apostrophe (« haven't » : « haven »), mot composé
            // (« smart-watches ») : chaque partie, la pire l'emporte.
            let mut rest = w.as_str();
            if self.lang == "fr" {
                if let Some(e) = ELISIONS.iter().find(|e| rest.starts_with(**e) && rest.len() > e.len()) {
                    rest = &rest[e.len()..];
                }
            }
            let parts: Vec<&str> = rest.split(['-', '\'']).filter(|p| !p.is_empty()).collect();
            let parts: Vec<&str> = if rest.contains('\'') && !rest.contains('-') { parts.into_iter().take(1).collect() } else { parts };
            if parts.is_empty() || (parts.len() == 1 && parts[0] == w) {
                whole
            } else {
                let kinds: Vec<Kind> = parts.iter().map(|p| self.class_of(p)).collect();
                if kinds.contains(&Kind::Unknown) {
                    Kind::Unknown
                } else if kinds.contains(&Kind::Foreign) {
                    Kind::Foreign
                } else {
                    Kind::Known
                }
            }
        };
        if kind == Kind::Unknown && trimmed.chars().next().is_some_and(char::is_uppercase) {
            Kind::Name
        } else {
            kind
        }
    }
}

/// Minuscules, apostrophe droite, ligatures défaites (« Œuvre » → « oeuvre »).
fn clean(w: &str) -> String {
    w.to_lowercase().replace(['’', '‘'], "'").replace('œ', "oe").replace('æ', "ae")
}

/// Clé de comparaison de deux transcriptions : lettres et chiffres seulement (« OK » = « ok. »,
/// « aujourd'hui » = « aujourd’hui »), « okay » = « ok ».
fn key(w: &str) -> String {
    let k: String = clean(w).chars().filter(|c| c.is_alphanumeric()).collect();
    if k == "okay" { "ok".to_string() } else { k }
}

fn text(w: &Value) -> &str {
    w.get("w").and_then(Value::as_str).unwrap_or("")
}

fn prob(w: &Value) -> f64 {
    w.get("p").and_then(Value::as_f64).unwrap_or(1.0)
}

/// Mot à relire : confiance basse, autre langue ou inconnu ; pas les hésitations et bruits
/// (« euh », « Ha, », « *cris* »), qu'une seconde écoute n'améliore pas.
fn suspect(w: &Value, dicts: Option<&Dicts>) -> bool {
    let k = key(text(w));
    if is_filler(&k) || matches!(k.as_str(), "ha" | "haha" | "hahaha" | "eh" | "hé" | "he") || text(w).starts_with(['*', '[', '(']) {
        return false;
    }
    prob(w) < DOUBT || dicts.is_some_and(|d| matches!(d.kind(text(w)), Kind::Foreign | Kind::Unknown))
}

/// Segment surtout dans une autre langue : au moins la moitié de ses mots (hors « euh »).
pub fn mostly_foreign(ws: &[Value], dicts: Option<&Dicts>) -> bool {
    let Some(d) = dicts else { return false };
    let kinds: Vec<Kind> = ws.iter().filter(|w| !is_filler(&key(text(w)))).map(|w| d.kind(text(w))).collect();
    !kinds.is_empty() && 2 * kinds.iter().filter(|k| **k == Kind::Foreign).count() >= kinds.len()
}

/// Mots suspects d'un segment, avec leur raison (pour le journal) ; aucun : pas de seconde écoute.
pub fn suspects(ws: &[Value], dicts: Option<&Dicts>) -> Vec<String> {
    ws.iter()
        .filter(|w| suspect(w, dicts))
        .map(|w| {
            let why = match dicts.map(|d| d.kind(text(w))) {
                Some(Kind::Foreign) => "autre langue",
                Some(Kind::Unknown) => "inconnu",
                _ => "confiance",
            };
            format!("{} ({why})", text(w))
        })
        .collect()
}

/// Ce qu'a changé la relecture d'un segment.
#[derive(Default, Debug, PartialEq)]
pub struct Stats {
    pub confirmed: usize, // mots suspects entendus pareil par Whisper
    pub replaced: usize,  // mots de Parakeet remplacés par ceux de Whisper
    pub proposed: usize,  // autres versions proposées sans remplacer
}

/// Compare les mots de Parakeet (`pk`, modifiés sur place) et de Whisper (`wh`) d'un segment.
pub fn merge(pk: &mut Vec<Value>, wh: &[Value], dicts: Option<&Dicts>) -> Stats {
    let mut stats = Stats::default();
    let sus: Vec<bool> = pk.iter().map(|w| suspect(w, dicts)).collect();
    // Comparaison sans « euh » ni signes : indices des mots comparés.
    let pi: Vec<usize> = (0..pk.len()).filter(|&k| !is_filler(&key(text(&pk[k])))).collect();
    let wi: Vec<usize> = (0..wh.len()).filter(|&k| !is_filler(&key(text(&wh[k])))).collect();
    let a: Vec<String> = pi.iter().map(|&k| key(text(&pk[k]))).collect();
    let b: Vec<String> = wi.iter().map(|&k| key(text(&wh[k]))).collect();
    let ops = SequenceMatcher::new(&a, &b).get_opcodes();
    let known_own = |w: &Value| dicts.is_none_or(|d| matches!(d.kind(text(w)), Kind::Known | Kind::Name));
    let foreign_word = |w: &Value| dicts.is_some_and(|d| d.kind(text(w)) == Kind::Foreign);
    // De la fin vers le début : un remplacement change le nombre de mots après lui seulement.
    for (tag, i1, i2, j1, j2) in ops.into_iter().rev() {
        match tag {
            Tag::Equal => {
                for &k in &pi[i1..i2] {
                    if sus[k] {
                        pk[k]["v"] = json!(0);
                        stats.confirmed += 1;
                    }
                }
            }
            Tag::Replace | Tag::Delete => {
                let (first, last) = (pi[i1], pi[i2 - 1]);
                if !(first..=last).any(|k| sus[k]) {
                    continue;
                }
                let theirs: Vec<&Value> = wi[j1..j2].iter().map(|&k| &wh[k]).collect();
                if invented(&theirs, last - first + 1) {
                    for k in first..=last {
                        pk[k]["v"] = json!(1); // doute gardé, rien à proposer
                    }
                    continue;
                }
                let theirs_text = theirs.iter().map(|w| text(w)).collect::<Vec<_>>().join(" ");
                let ours_text = pk[first..=last].iter().map(text).collect::<Vec<_>>().join(" ");
                let n = last - first + 1;
                let take = tag == Tag::Replace
                    && (first..=last).any(|k| foreign_word(&pk[k]))
                    && theirs.iter().all(|w| known_own(w));
                if take {
                    let (a0, b0) = (num(&pk[first], "a"), num(&pk[last], "b"));
                    let weights: Vec<f64> = theirs.iter().map(|w| text(w).chars().filter(|c| c.is_alphanumeric()).count().max(1) as f64).collect();
                    let total: f64 = weights.iter().sum();
                    let mut t = a0;
                    let mut new: Vec<Value> = vec![];
                    for (w, wt) in theirs.iter().zip(&weights) {
                        let end = t + (b0 - a0) * wt / total;
                        new.push(json!({"w": text(w), "a": round_to(t, 3), "b": round_to(end, 3), "p": round_to(prob(w), 2), "v": 1}));
                        t = end;
                    }
                    new[0]["alt"] = json!(ours_text);
                    if new.len() > 1 {
                        new[0]["altn"] = json!(new.len());
                    }
                    stats.replaced += n;
                    pk.splice(first..=last, new);
                } else {
                    for k in first..=last {
                        pk[k]["v"] = json!(1);
                    }
                    pk[first]["alt"] = json!(theirs_text);
                    if n > 1 {
                        pk[first]["altn"] = json!(n);
                    }
                    stats.proposed += 1;
                }
            }
            Tag::Insert => {
                if j2 - j1 < LOST_WORDS || pi.is_empty() {
                    continue;
                }
                let found: Vec<&Value> = wi[j1..j2].iter().map(|&k| &wh[k]).collect();
                if invented(&found, 0) {
                    continue;
                }
                let lost = found.iter().map(|w| text(w)).collect::<Vec<_>>().join(" ");
                // Sur le mot d'avant (ou le premier mot si les mots perdus ouvrent le segment).
                let (k, alt) = if i1 > 0 {
                    let k = pi[i1 - 1];
                    (k, format!("{} {lost}", text(&pk[k])))
                } else {
                    let k = pi[0];
                    (k, format!("{lost} {}", text(&pk[k])))
                };
                if pk[k].get("alt").is_some() {
                    continue;
                }
                pk[k]["alt"] = json!(alt);
                pk[k]["v"] = json!(1);
                stats.proposed += 1;
            }
        }
    }
    stats
}

/// Version de Whisper qui ressemble à une invention : bien plus de mots que ceux qu'elle remplacerait
/// (plus de 2 n + 3), ou un même mot répété au moins 3 fois (« latin, latin, latin »).
fn invented(theirs: &[&Value], n: usize) -> bool {
    let keys: Vec<String> = theirs.iter().map(|w| key(text(w))).collect();
    keys.len() > 2 * n + 3 || keys.iter().any(|k| keys.iter().filter(|x| *x == k).count() >= 3)
}

fn num(w: &Value, k: &str) -> f64 {
    w.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dicts() -> Dicts {
        Dicts::from_counts(
            "fr",
            &[("des", 1000.0), ("montres", 50.0), ("intelligentes", 20.0), ("on", 900.0), ("a", 1000.0), ("smartphones", 0.0), ("information", 30.0), ("and", 1.0), ("peut-être", 40.0), ("par", 500.0), ("exemple", 80.0), ("sur", 400.0), ("un", 900.0), ("gâteau", 10.0), ("monts", 5.0), ("c'est", 900.0), ("bon", 300.0), ("une", 900.0), ("minute", 100.0), ("voilà", 200.0), ("one", 2.0)],
            &[("and", 1000.0), ("one", 3000.0), ("the", 2000.0), ("information", 30.0), ("on", 800.0), ("a", 1500.0), ("smartphones", 5.0), ("watches", 10.0)],
        )
    }

    fn words(text: &str, p: f64) -> Vec<Value> {
        text.split(' ').enumerate().map(|(k, w)| json!({"w": w, "a": k as f64, "b": k as f64 + 1.0, "p": p})).collect()
    }

    #[test]
    fn kinds() {
        let d = dicts();
        assert_eq!(d.kind("and"), Kind::Foreign);
        assert_eq!(d.kind("The"), Kind::Foreign);
        assert_eq!(d.kind("information,"), Kind::Known);
        assert_eq!(d.kind("l'information"), Kind::Known);
        assert_eq!(d.kind("smartwatchs"), Kind::Unknown);
        assert_eq!(d.kind("Bashunda"), Kind::Name);
        assert_eq!(d.kind("Okay."), Kind::Known);
        assert_eq!(d.kind("euh"), Kind::Known);
        assert_eq!(d.kind("2023"), Kind::Known);
        assert_eq!(d.kind("Peut-être"), Kind::Known);
        assert_eq!(d.kind("smart-watches"), Kind::Unknown);
        assert_eq!(d.kind("One"), Kind::Foreign);
    }

    #[test]
    fn confirmed_by_whisper() {
        let mut pk = words("on a des montres", 0.3);
        let st = merge(&mut pk, &words("On a des montres.", 0.9), Some(&dicts()));
        assert_eq!(st.confirmed, 4);
        assert!(pk.iter().all(|w| w["v"] == 0));
    }

    #[test]
    fn foreign_word_replaced() {
        // « One » (anglais) dans une phrase française : version de Whisper, celle de Parakeet proposée.
        let mut pk = words("C'est bon. One minute. Voilà.", 0.9);
        let st = merge(&mut pk, &words("C'est bon. Une minute. Voilà.", 0.8), Some(&dicts()));
        assert_eq!(text(&pk[2]), "Une");
        assert_eq!(pk[2]["alt"], "One");
        assert_eq!(pk[2]["v"], 1);
        assert!(pk[2].get("altn").is_none());
        assert_eq!(st.replaced, 1);
        assert_eq!((pk[2]["a"].as_f64(), pk[2]["b"].as_f64()), (Some(2.0), Some(3.0)));
    }

    #[test]
    fn unknown_words_proposed() {
        // Mots inconnus (« smartwatchs ») : gardés, version de Whisper proposée sur tout l'écart.
        let mut pk = words("des smartwatchs, des monts intelligents", 0.9);
        let st = merge(&mut pk, &words("des montres intelligentes", 0.8), Some(&dicts()));
        assert_eq!(text(&pk[1]), "smartwatchs,");
        assert_eq!(pk[1]["alt"], "montres intelligentes");
        assert_eq!(pk[1]["altn"], 4);
        assert!((1..5).all(|k| pk[k]["v"] == 1));
        assert_eq!(st.proposed, 1);
    }

    #[test]
    fn kept_when_whisper_not_better() {
        // Mot douteux mais connu : Parakeet gardé, Whisper proposé.
        let mut pk = words("un gâteau", 0.9);
        pk[1]["p"] = json!(0.2);
        let st = merge(&mut pk, &words("un bateau", 0.9), Some(&dicts()));
        assert_eq!(text(&pk[1]), "gâteau");
        assert_eq!(pk[1]["alt"], "bateau");
        assert_eq!(pk[1]["v"], 1);
        assert_eq!(st.proposed, 1);
    }

    #[test]
    fn differences_far_from_doubts_left() {
        let mut pk = words("un gâteau sur des montres", 0.9);
        pk[4]["p"] = json!(0.2);
        merge(&mut pk, &words("un bateau sur des montres", 0.9), Some(&dicts()));
        assert!(pk[1].get("alt").is_none());
        assert_eq!(pk[4]["v"], 0);
    }

    #[test]
    fn inventions_left() {
        let mut pk = words("qu'une", 0.3);
        merge(&mut pk, &words("lat in, latin, latin, latin", 0.9), Some(&dicts()));
        assert!(pk[0].get("alt").is_none());
        assert_eq!(pk[0]["v"], 1);
    }

    #[test]
    fn lost_words_proposed() {
        let mut pk = words("un gâteau", 0.9);
        pk[1]["p"] = json!(0.3);
        merge(&mut pk, &words("par exemple sur un gâteau", 0.9), Some(&dicts()));
        assert_eq!(pk[0]["alt"], "par exemple sur un");
        assert_eq!(pk[1]["v"], 0);
        // Un seul mot : laissé.
        let mut pk = words("un gâteau", 0.3);
        merge(&mut pk, &words("euh sur un gâteau", 0.9), Some(&dicts()));
        assert!(pk[0].get("alt").is_none());
    }
}
