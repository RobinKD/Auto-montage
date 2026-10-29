//! Redites repérées sans Claude : faux départs, 1res prises reprises juste après, hésitations et
//! apartés, au mot près.
//!
//! Usage : auto-montage redites            coupe les moments aux faux départs (work/decoupage.json,
//!                                         seulement s'il n'existe pas : nouveau rush)
//!         auto-montage redites --essai    montre ce qui serait retiré, sans rien écrire
//!
//! Les mots de tout le rush sont lus à la suite (work/segments.json), sans les « euh ». Une reprise
//! est un endroit où la parole repart sur les mêmes mots qu'un peu avant (45 s au plus) : le passage
//! entre les deux départs est la prise abandonnée, retirée ; la dernière prise est gardée.
//! Trois formes (`detect`) :
//!   - bégaiement : 2 mots ou plus redits aussitôt, rien entre (« je vais, je vais vous montrer ») ;
//!   - faux départ : 3 mots ou plus repris (dont un mot plein, ou 5 mots), la 1re prise
//!     s'arrêtant au plus 8 mots après, sans finir de phrase avant (« unité de mesure. Une nouvelle
//!     unité de mesure… » est voulu) ;
//!   - 1re prise entière : la suivante lui ressemble à 70 % ou plus.
//! Contre les phrases simplement construites pareil (« on va voir X. On va voir Y. »), les mots
//! pleins de la 1re prise doivent se retrouver dans la suivante (un seul peut manquer si la 1re prise
//! en a 3 ou plus et n'est pas une phrase finie : mot corrigé en reprenant).
//! Un moment fait seulement d'hésitations (« euh ») ou d'un aparté (« attends, je recommence »)
//! est aussi retiré (`moment_reason`).
//!
//! Sur la page des moments, un moment dont tous les mots sont dans une prise abandonnée est
//! suggéré retiré (moments_lib::suggested_keep) ; prepare lance cette commande après la
//! transcription pour couper les moments là où commence et finit une prise abandonnée, si bien que
//! le reste du moment est gardé (« Recoller » défait la coupe).

use crate::moments_lib::{effective_segments, load_decoupage, save_decoupage, Decoupage};
use crate::paths;
use crate::util;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// Écart maximal entre les deux départs (s), et en mots.
const WINDOW_S: f64 = 45.0;
const MAX_BACK: usize = 150;
/// Faux départ : mots de la 1re prise après la partie reprise, au plus.
const MAX_TAIL: usize = 8;
/// 1re prise entière : ressemblance minimale avec la suivante.
const MIN_RATIO: f64 = 0.7;

pub const RETAKE: &str = "1re prise, reprise juste après";
pub const FALSE_START: &str = "faux départ, repris juste après";
pub const HESITATION: &str = "hésitation";
pub const ASIDE: &str = "aparté";

/// Mots outils (français, anglais) : ne suffisent pas à faire une reprise.
static STOP: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "le la les l un une des du de d au aux a à et ou mais donc or ni car que qu qui quoi dont ou où \
     ce c cet cette ces ca ça cela ceci il ils elle elles on je j tu nous vous me m te t se s lui leur \
     leurs y en ne n pas plus moins tres très bien tout tous toute toutes mon ma mes ton ta tes son sa \
     ses notre nos votre vos est es suis sont sommes etes êtes ete été etre être ai as avons avez ont \
     avoir fait faire va vais vas vont allons allez aller dans par pour sur sous avec sans chez entre \
     vers comme si alors puis aussi encore deja déjà meme même voila voilà ben bon bah oui non ok okay \
     quand comment pourquoi parce peu fois ici la là the a an and or but so of to in on at by for with \
     from as is are was were be been am i you he she it we they me him her us them my your his its our \
     their this that these those there here what which who how why when do does did have has had will \
     would can could should not no yes just very really then also well like gonna going get got"
        .split_whitespace()
        .collect()
});

/// Mots d'un aparté, et ceux qui en font un (un moment fait seulement de mots d'aparté, dont un
/// déclencheur, est retiré).
static ASIDE_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "attends attendez pardon excuse excusez excuse-moi excusez-moi desole désolé desolee désolée moi \
     je j on recommence recommencer recommence reprends reprend reprendre refais refait refaire la le \
     l c c'est est bon ok okay ca ça tourne coupe coupez zut mince merde oups non oh la là encore une \
     fois again sorry wait hang let me start over one more time take oops cut i'll i'm it that s again \
     so un deux trois go vas-y allez enfin plutôt plutot"
        .split_whitespace()
        .collect()
});
static ASIDE_TRIGGERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "attends attendez pardon excuse excusez excuse-moi excusez-moi desole désolé desolee désolée \
     recommence recommencer reprends refais refait refaire coupe coupez zut mince merde oups sorry \
     wait again oops cut"
        .split_whitespace()
        .collect()
});

/// Mot comparable : minuscules, apostrophe droite, sans ponctuation autour.
fn norm(w: &str) -> String {
    let w = w.to_lowercase().replace('’', "'");
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// Forme d'un mot pour les comparer : sans accents ni marque du pluriel (« ça » et « ca »,
/// « prise » et « prises » se valent).
fn stem(w: &str) -> String {
    let mut w: String = w.nfd().filter(|c| c.is_alphanumeric() || *c == '\'').collect();
    if w.chars().count() > 3 && (w.ends_with('s') || w.ends_with('x')) {
        w.pop();
    }
    w
}

/// Hésitation : euh, heu, hum, hmm, mmh, uh, um, erm…
pub fn is_filler(n: &str) -> bool {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^(e+u+h*|h+e+u+h*|h*u+m+|h*m+h*|u+h+|e+r+m+|a+h+|o+h+|bah|ben)$").unwrap());
    n.is_empty() || RE.is_match(n)
}

fn is_content(n: &str) -> bool {
    n.chars().count() >= 3 && !STOP.contains(n) && !STOP.contains(n.rsplit('\'').next().unwrap_or(n))
}

/// Fin de phrase (point, point d'exclamation ou d'interrogation, sans points de suspension).
fn ends_sentence(w: &str) -> bool {
    let w = w.trim_end();
    (w.ends_with('.') || w.ends_with('!') || w.ends_with('?')) && !w.ends_with("..") && !w.ends_with('…')
}

struct Tok {
    idx: usize, // rang dans la liste de tous les mots
    aside: bool, // mot d'aparté ou de correction (« attends », « je recommence », « enfin »)
    stem: String,
    content: bool,
    a: f64,
    end_sentence: bool,
}

/// Mots retirés : pour chaque mot (texte, début en s), la raison de son retrait, ou None.
pub fn detect(words: &[(String, f64)]) -> Vec<Option<&'static str>> {
    let mut out: Vec<Option<&'static str>> = vec![None; words.len()];
    let s: Vec<Tok> = words
        .iter()
        .enumerate()
        .filter_map(|(idx, (w, a))| {
            let n = norm(w);
            let aside = ASIDE_WORDS.contains(n.as_str());
            (!is_filler(&n)).then(|| Tok { idx, aside, stem: stem(&n), content: is_content(&n) && !aside, a: *a, end_sentence: ends_sentence(w) })
        })
        .collect();
    let stems: Vec<&str> = s.iter().map(|t| t.stem.as_str()).collect();
    let mut j = 1;
    while j < s.len() {
        // (mots repris, début de la 1re prise, raison)
        let mut best: Option<(usize, usize, &'static str)> = None;
        for i in (j.saturating_sub(MAX_BACK)..j).rev() {
            if s[j].a - s[i].a > WINDOW_S {
                break;
            }
            if s[i].stem != s[j].stem {
                continue;
            }
            let mut n = 0;
            while i + n < j && j + n < s.len() && s[i + n].stem == s[j + n].stem {
                n += 1;
            }
            let len = j - i; // mots de la 1re prise
            let retake = len >= 4 && s[i..j].iter().any(|t| t.content) && same_content(&s, i, j) && {
                let b = &stems[j..(j + len + 2).min(s.len())];
                crate::difflib::SequenceMatcher::new(&stems[i..j], b).ratio() >= MIN_RATIO
            };
            let reason = if retake {
                Some(RETAKE)
            } else if n >= 2 && len == n {
                Some(FALSE_START) // bégaiement
            } else if n >= 3
                && len - n <= MAX_TAIL
                && (n >= 5 || s[i..i + n].iter().any(|t| t.content))
                && !core(&s[i..j]).split_last().map(|(_, r)| r.iter().any(|t| t.end_sentence)).unwrap_or(false)
                && same_content(&s, i, j)
            {
                Some(FALSE_START)
            } else {
                None
            };
            if let Some(r) = reason {
                if best.map(|(bn, _, _)| n > bn).unwrap_or(true) {
                    best = Some((n, i, r));
                }
            }
        }
        if let Some((n, i, r)) = best {
            // Toute la 1re prise, « euh » compris, jusqu'au mot où la parole repart.
            for k in s[i].idx..s[j].idx {
                if out[k].is_none() || r == RETAKE {
                    out[k] = Some(r);
                }
            }
            // Les mots repris ne sont pas de nouveaux départs (sinon « on va » de la 2e prise
            // serait pris dans une reprise décalée d'un mot).
            j += n;
        } else {
            j += 1;
        }
    }
    out
}

/// Les mots pleins de la 1re prise (s[i..j]) se retrouvent dans la suite (s[j..]) : tous si la
/// 1re prise finit une phrase ou en a moins de 3, tous sauf un sinon (mot corrigé en reprenant).
/// Les mots d'aparté qui la terminent (« attends, je recommence ») ne comptent pas.
fn same_content(s: &[Tok], i: usize, j: usize) -> bool {
    let len = j - i;
    let after: HashSet<&str> = s[j..(j + len + 6).min(s.len())].iter().map(|t| t.stem.as_str()).collect();
    let content: BTreeSet<&str> = s[i..j].iter().filter(|t| t.content).map(|t| t.stem.as_str()).collect();
    let missing = content.iter().filter(|c| !after.contains(*c)).count();
    let ends = core(&s[i..j]).last().map(|t| t.end_sentence).unwrap_or(false);
    let allowed = if ends || content.len() < 3 { 0 } else { 1 };
    missing <= allowed
}

/// Mots d'une prise sans ceux d'aparté (« attends, je recommence ») qui la terminent.
fn core(take: &[Tok]) -> &[Tok] {
    let end = take.iter().rposition(|t| !t.aside).map(|k| k + 1).unwrap_or(0);
    &take[..end]
}

/// Moment fait seulement d'hésitations, ou d'un aparté (« attends, je recommence »).
pub fn moment_reason(words: &[&str]) -> Option<&'static str> {
    let ns: Vec<String> = words.iter().map(|w| norm(w)).collect();
    if ns.is_empty() {
        return None; // parole sans transcription : gardée
    }
    let rest: Vec<&String> = ns.iter().filter(|n| !is_filler(n)).collect();
    if rest.is_empty() {
        return Some(HESITATION);
    }
    if rest.iter().all(|n| ASIDE_WORDS.contains(n.as_str())) && rest.iter().any(|n| ASIDE_TRIGGERS.contains(n.as_str())) {
        return Some(ASIDE);
    }
    None
}

fn word_list(segments: &[Value]) -> Vec<(String, f64)> {
    segments
        .iter()
        .flat_map(|s| util::arr(s, "words"))
        .map(|w| (w.get("w").and_then(Value::as_str).unwrap_or("").to_string(), w.get("a").and_then(Value::as_f64).unwrap_or(0.0)))
        .collect()
}

/// Raison du retrait de chaque moment (moments effectifs, dans l'ordre du rush) : tous ses mots
/// (hors « euh ») dans des prises abandonnées, ou hésitation, ou aparté.
pub fn suggestions(effective: &[Value]) -> BTreeMap<i64, &'static str> {
    let flags = detect(&word_list(effective));
    let mut out = BTreeMap::new();
    let mut k = 0;
    for m in effective {
        let ws = util::arr(m, "words");
        let texts: Vec<&str> = ws.iter().map(|w| w.get("w").and_then(Value::as_str).unwrap_or("")).collect();
        let fl = &flags[k..k + ws.len()];
        k += ws.len();
        let id = m.get("id").and_then(Value::as_i64).unwrap_or(0);
        let real: Vec<usize> = (0..texts.len()).filter(|&i| !is_filler(&norm(texts[i]))).collect();
        if let Some(r) = moment_reason(&texts) {
            out.insert(id, r);
        } else if !real.is_empty() && real.iter().all(|&i| fl[i].is_some()) {
            out.insert(id, if real.iter().any(|&i| fl[i] == Some(RETAKE)) { RETAKE } else { FALSE_START });
        }
    }
    out
}

/// Coupes à faire dans les segments (id -> rangs des mots où commence une partie) : au début et à
/// la fin de chaque prise abandonnée qui ne couvre qu'une partie d'un segment, si elle compte au
/// moins 2 mots (hors « euh »).
pub fn splits(segments: &[Value]) -> BTreeMap<String, Vec<i64>> {
    let words = word_list(segments);
    let flags = detect(&words);
    let mut out = BTreeMap::new();
    let mut k = 0;
    for seg in segments {
        let n = util::arr(seg, "words").len();
        let fl = &flags[k..k + n];
        let real = |a: usize, b: usize| (a..b).filter(|&i| !is_filler(&norm(&words[k + i].0))).count();
        let mut cuts: BTreeSet<i64> = BTreeSet::new();
        let mut i = 0;
        while i < n {
            if fl[i].is_none() {
                i += 1;
                continue;
            }
            let start = i;
            while i < n && fl[i].is_some() {
                i += 1;
            }
            if (start > 0 || i < n) && real(start, i) >= 2 {
                for c in [start, i] {
                    if 0 < c && c < n {
                        cuts.insert(c as i64);
                    }
                }
            }
        }
        k += n;
        if !cuts.is_empty() {
            out.insert(seg.get("id").and_then(Value::as_i64).unwrap_or(0).to_string(), cuts.into_iter().collect());
        }
    }
    out
}

fn excerpt(words: &[(String, f64)], a: usize, b: usize) -> String {
    words[a..b].iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(" ")
}

/// `auto-montage redites [--essai]`
pub fn main(args: &[String]) -> i32 {
    let work = paths::work();
    let essai = args.iter().any(|a| a == "--essai");
    let segments = match util::read_json(work.join("segments.json")) {
        Some(Value::Array(a)) => a,
        _ => {
            eprintln!("work/segments.json illisible");
            return 1;
        }
    };
    let words = word_list(&segments);
    let flags = detect(&words);
    let (mut count, mut seconds) = (0, 0.0);
    let mut i = 0;
    while i < flags.len() {
        let Some(r) = flags[i] else {
            i += 1;
            continue;
        };
        let start = i;
        while i < flags.len() && flags[i] == Some(r) {
            i += 1;
        }
        count += 1;
        let end = words.get(i).map(|w| w.1).unwrap_or(words[i - 1].1);
        seconds += (end - words[start].1).max(0.0);
        if essai {
            println!("{:>8.2} s  {r} : « {} »  →  « {} »", words[start].1, excerpt(&words, start, i), excerpt(&words, i, (i + 8).min(words.len())));
        }
    }
    let deco = load_decoupage(&work);
    let eff = effective_segments(&segments, &deco);
    let whole = suggestions(&eff);
    for (id, r) in &whole {
        if essai && (*r == HESITATION || *r == ASIDE) {
            let m = eff.iter().find(|m| m.get("id").and_then(Value::as_i64) == Some(*id));
            println!("moment {id}  {r} : « {} »", m.and_then(|m| m.get("text")).and_then(Value::as_str).unwrap_or(""));
        }
    }
    println!("Redites : {count} prise(s) abandonnée(s) repérée(s), {seconds:.1} s ; {} moment(s) à retirer.", whole.len());
    if essai {
        return 0;
    }
    if work.join("decoupage.json").exists() {
        println!("work/decoupage.json existe déjà (moments coupés à la main) : pas de coupe automatique.");
        return 0;
    }
    let sp = splits(&segments);
    if !sp.is_empty() {
        let cuts: usize = sp.values().map(Vec::len).sum();
        save_decoupage(&work, &Decoupage { splits: sp });
        println!("Moments coupés aux faux départs : {cuts} coupe(s) (work/decoupage.json).");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mots d'un texte, un toutes les 0,3 s ; « / » marque une pause d'1 s.
    fn ws(text: &str) -> Vec<(String, f64)> {
        let mut t = 0.0;
        let mut out = vec![];
        for w in text.split_whitespace() {
            if w == "/" {
                t += 1.0;
                continue;
            }
            out.push((w.to_string(), t));
            t += 0.3;
        }
        out
    }

    fn removed(text: &str) -> String {
        let w = ws(text);
        let f = detect(&w);
        w.iter().zip(f).filter(|(_, f)| f.is_some()).map(|(w, _)| w.0.as_str()).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn faux_depart() {
        assert_eq!(removed("Bonjour. Aujourd'hui on va parler / euh / aujourd'hui on va parler du montage."), "Aujourd'hui on va parler euh");
        assert_eq!(removed("je vais, je vais vous montrer le logiciel"), "je vais,");
        // mot corrigé en reprenant
        assert_eq!(
            removed("on va écrire le programme en Python / non / on va écrire le programme en Rust parce que"),
            "on va écrire le programme en Python non"
        );
    }

    #[test]
    fn premiere_prise_entiere() {
        assert_eq!(
            removed("Le montage automatique coupe les silences et garde les meilleures prises. / Bon. / Le montage automatique coupe tous les silences et garde les meilleures prises."),
            "Le montage automatique coupe les silences et garde les meilleures prises. Bon."
        );
        // prise interrompue par un aparté
        assert_eq!(
            removed("La première étape c'est de choisir le bon rush, enfin le bon / Attends. Je recommence. / La première étape, c'est de choisir le bon rush."),
            "La première étape c'est de choisir le bon rush, enfin le bon Attends. Je recommence."
        );
        // trois prises : seule la dernière reste
        assert_eq!(removed("Tu ouvres la page / tu ouvres la fenêtre / tu ouvres la page des moments"), "Tu ouvres la page tu ouvres la fenêtre");
    }

    #[test]
    fn phrases_construites_pareil_gardees() {
        assert_eq!(removed("On va voir comment couper une vidéo. On va voir comment ajouter des sous-titres."), "");
        assert_eq!(removed("C'est pour ça que j'aime le vélo. C'est pour ça que je roule tous les jours."), "");
        assert_eq!(removed("nous nous sommes levés tôt"), "");
        assert_eq!(removed("le prix de la pomme de la région"), "");
        // phrases du rush Wikimania (Parakeet)
        assert_eq!(removed("définir l'information comme unité de mesure. Une nouvelle unité de mesure doit être connue par nous"), "");
        assert_eq!(removed("Maintenant, on a des smartphones des smartwatchs, des montres intelligentes."), "");
        assert_eq!(removed("tu cliques sur la page des moments puis sur la page des réglages"), "");
        // trop loin (plus de 45 s)
        let mut w = ws("on va parler du montage");
        w.extend(ws("on va parler du montage vidéo").into_iter().map(|(x, t)| (x, t + 60.0)));
        assert!(detect(&w).iter().all(Option::is_none));
    }

    #[test]
    fn hesitations_et_apartes() {
        assert_eq!(moment_reason(&["Euh…"]), Some(HESITATION));
        assert_eq!(moment_reason(&["Attends,", "je", "recommence."]), Some(ASIDE));
        assert_eq!(moment_reason(&["Pardon."]), Some(ASIDE));
        assert_eq!(moment_reason(&["Non."]), None);
        assert_eq!(moment_reason(&["Bon,", "on", "commence."]), None);
        assert_eq!(moment_reason(&[]), None);
    }

    #[test]
    fn coupes_dans_les_segments() {
        use serde_json::json;
        let seg = |id: i64, text: &str, t0: f64| {
            let words: Vec<Value> = ws(text).into_iter().map(|(w, a)| json!({"w": w, "a": a + t0, "b": a + t0 + 0.25})).collect();
            json!({"id": id, "start": t0, "end": t0 + 3.0, "text": text, "words": words})
        };
        // faux départ en tête du 2e segment : coupé, le reste du segment gardé
        let segs = vec![seg(0, "Bonjour à tous.", 0.0), seg(1, "Je vais vous je vais vous montrer le logiciel", 5.0)];
        let sp = splits(&segs);
        assert_eq!(sp.get("1"), Some(&vec![3]));
        let eff = effective_segments(&segs, &Decoupage { splits: sp });
        let sug = suggestions(&eff);
        assert_eq!(sug.get(&1), Some(&FALSE_START));
        assert_eq!(sug.len(), 1);
        // 1re prise dans son propre segment : retiré entier, sans coupe
        let segs = vec![seg(0, "On va parler du montage", 0.0), seg(1, "on va parler du montage automatique", 4.0)];
        assert!(splits(&segs).is_empty());
        let sug = suggestions(&effective_segments(&segs, &Decoupage::default()));
        assert_eq!(sug.keys().collect::<Vec<_>>(), vec![&0]);
    }
}
