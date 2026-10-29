//! Travaux confiés à Claude Code sans interaction (« claude -p »), lancés par l'interface locale.
//!
//! `auto-montage apply_instructions` : applique les consignes de montage (work/instructions.md)
//! au rush en cours. Claude lit la transcription et écrit work/suggestions.json (moments à
//! garder, avec raisons) et work/edit_choices.json (coupes, corrections, voix modifiée, caisse,
//! pastilles, intro), que make_moments et build_edit utilisent ensuite. Claude Code connecté :
//! « claude » une fois, ou ANTHROPIC_API_KEY. Lancé par la préparation de la page « Rushes et
//! montages » quand on le demande. Éléments de style : sons personnels (public/sfx/perso/) et
//! planches des vidéos d'exemple (work/style/videos/*.planche.jpg) ; analyse écrite dans
//! work/analyse_style.md.
//!
//! `auto-montage style_claude` : analyse des vidéos d'exemple, seconde étape. Claude regarde les
//! planches datées et les mesures d'analyze_style (work/style/analyse/faits.json) et écrit ce
//! qu'il a repéré, et ce qu'Auto-montage peut en reproduire, dans work/style/analyse/resultat.json
//! (affiché sur la page « Rushes et montages ») et work/analyse_style.md (résumé lu à la
//! préparation et montré sur la page des moments).
//!
//! Écriture limitée à montage/work/ (pas de mode acceptEdits, qui accepterait toute
//! modification du projet) : un texte piégé dans le rush ou les consignes ne peut pas faire
//! réécrire un script que Claude lancerait ensuite. Les seules commandes permises sont des
//! commandes d'auto-montage (build_edit, fonts_lib) : le dossier de ce programme est mis en tête
//! du PATH de Claude pour qu'il les trouve sous le nom « auto-montage ».

use crate::moments_lib::{hidden_effects, language_from, language_label};
use crate::paths;
use crate::procs;
use crate::util;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Fichiers de montage/work/ que Claude peut écrire (consignes, discussion, analyse du style) :
/// ceux que ses consignes lui demandent, et les réglages de la page. Rien d'autre : ni les pages
/// servies et leurs scripts, ni la conversation, ni les montages enregistrés (work/projets/, que
/// « project open » remet en place), ni la tâche en pause (work/tache.json, relancée par la
/// reprise). Noms exacts plutôt qu'un motif et des interdits : sur un disque qui ne distingue pas
/// les majuscules, « TACHE.json » échapperait à un interdit « tache.json », pas à cette liste.
pub const WRITABLE: [&str; 14] = [
    "suggestions.json",
    "edit_choices.json",
    "caption_style.json",
    "habillage.json",
    "son.json",
    "couleur.json",
    "transitions.json",
    "effets_page.json",
    "decoupage.json",
    "derush.json",
    "instructions.md",
    "analyse_style.md",
    "local_db/**/*.json",
    "style/analyse/resultat.json",
];

/// Interdits d'écriture gardés en plus de la liste : conversation et pages servies.
pub fn denied_rules() -> Vec<String> {
    let mut v = vec![];
    for t in ["Edit", "Write"] {
        for p in ["montage/work/chat/**", "montage/work/**/*.html", "montage/work/**/*.js"] {
            v.push(format!("{t}({p})"));
        }
    }
    v
}

/// Dossier montage/ tel que l'écrivent les règles de Claude Code : « montage » (relatif au
/// dossier où Claude est lancé), puis son chemin absolu (« //home/… », « //c/Users/… » sous
/// Windows), qui vaut encore après un « cd montage » de Claude. Chemin absolu laissé de côté
/// s'il contient des caractères de motif ou de liste (règle mal comprise).
fn rule_roots() -> Vec<String> {
    let mut v = vec!["montage".to_string()];
    let root = paths::root();
    let mut forms: Vec<PathBuf> = vec![];
    if let Ok(p) = std::path::absolute(&root) {
        forms.push(p);
    }
    if let Ok(p) = std::fs::canonicalize(&root) {
        forms.push(p);
    }
    for p in forms {
        let mut s = p.to_string_lossy().replace('\\', "/");
        if let Some(rest) = s.strip_prefix("//?/") {
            s = rest.to_string();
        }
        let b = s.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
            s = format!("/{}{}", (b[0] as char).to_ascii_lowercase(), &s[2..]);
        }
        let s = s.trim_end_matches('/');
        if !s.starts_with('/') || s.chars().any(|c| "*?[]{}()!,\\\"'\n\r\t".contains(c)) {
            continue;
        }
        let rule = format!("/{s}");
        if !v.contains(&rule) {
            v.push(rule);
        }
    }
    v
}

/// Lecture (Read, Glob, Grep) limitée au dossier montage/ (le dossier où Claude est lancé reste
/// lisible, comme toujours avec Claude Code) : plus de lecture ailleurs sur le disque.
pub fn read_rules() -> Vec<String> {
    let mut v = vec![];
    for r in rule_roots() {
        for t in ["Read", "Glob", "Grep"] {
            v.push(format!("{t}({r}/**)"));
        }
    }
    v
}

/// Écriture limitée aux fichiers de WRITABLE.
pub fn write_rules() -> Vec<String> {
    let mut v = vec![];
    for r in rule_roots() {
        for f in WRITABLE {
            for t in ["Edit", "Write"] {
                v.push(format!("{t}({r}/work/{f})"));
            }
        }
    }
    v
}

/// Options qui fixent les permissions de « claude -p », quels que soient les réglages de
/// l'utilisateur dans ~/.claude (mode « accepter les modifications » ou « sans permission »,
/// serveurs MCP) : mode par défaut (ce qui n'est pas permis est refusé, personne ne valide),
/// pas de serveur MCP, et seulement les outils utiles (pas de web). Chaque option n'est passée
/// que si la version de Claude Code installée la connaît (« claude --help », lu une fois).
pub fn pinned_args(claude: &Path) -> Vec<String> {
    static HELP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let help = HELP.get_or_init(|| {
        let mut c = Command::new(claude);
        c.arg("--help").stdin(std::process::Stdio::null());
        c.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
    });
    let mut v: Vec<String> = vec![];
    if help.contains("--permission-mode") {
        // « default » s'appelle « manual » dans les versions récentes (l'ancien nom reste accepté)
        let mode = if help.contains("\"default\"") || !help.contains("\"manual\"") { "default" } else { "manual" };
        v.extend(["--permission-mode".into(), mode.into()]);
    }
    if help.contains("--strict-mcp-config") {
        v.push("--strict-mcp-config".into());
    }
    if help.contains("--tools ") {
        v.push("--tools".into());
        v.extend(["Read", "Glob", "Grep", "Edit", "Write", "Bash", "Skill", "TodoWrite"].iter().map(|s| s.to_string()));
    }
    v
}

/// Fichiers d'un dossier dont le nom finit par `suffix` (comme `ls dossier/*suffix`, sans les
/// fichiers cachés), triés.
fn listing(dir: &Path, suffix: &str) -> Vec<String> {
    util::list_dir(dir).into_iter().filter(|n| !n.starts_with('.') && n.ends_with(suffix) && dir.join(n).is_file()).collect()
}

/// Où est décrit le format d'un fichier de réglages : en tête du code de la commande.
fn described_in(name: &str) -> String {
    format!("montage/rust/src/{name}.rs")
}

/// Lance « claude -p » depuis le dossier qui contient montage/, avec auto-montage dans le PATH ;
/// renvoie son code de sortie. Permis : lire montage/, écrire les fichiers de WRITABLE, et les
/// commandes `allowed`.
fn run_claude(prompt: &str, allowed: &[&str]) -> i32 {
    let Some(claude) = paths::claude() else {
        eprintln!("Claude Code n'est pas installé.");
        return 1;
    };
    let mut path_dirs: Vec<PathBuf> = vec![];
    if let Some(d) = paths::self_exe().parent() {
        path_dirs.push(d.to_path_buf());
    }
    if let Some(p) = std::env::var_os("PATH") {
        path_dirs.extend(std::env::split_paths(&p));
    }
    let pinned = pinned_args(&claude);
    let mut cmd = Command::new(claude);
    cmd.arg("-p").arg(prompt).args(pinned);
    cmd.arg("--allowedTools").args(read_rules()).args(write_rules()).args(allowed);
    cmd.arg("--disallowedTools").args(denied_rules());
    if let Ok(p) = std::env::join_paths(path_dirs) {
        cmd.env("PATH", p);
    }
    let parent = paths::root().parent().map(Path::to_path_buf).unwrap_or_else(|| paths::root().to_path_buf());
    match cmd.current_dir(parent).status() {
        Ok(s) => {
            let c = procs::py_code(&s);
            if c < 0 {
                128 - c
            } else {
                c
            }
        }
        Err(e) => {
            eprintln!("Claude Code ne se lance pas : {e}");
            127
        }
    }
}

fn fail(msg: &str) -> i32 {
    eprintln!("{msg}");
    1
}

/// `auto-montage apply_instructions`
pub fn apply_instructions(_args: &[String]) -> i32 {
    let work = paths::work();
    let has_text = std::fs::metadata(work.join("instructions.md")).map(|m| m.len() > 0).unwrap_or(false);
    let sounds: Vec<String> =
        listing(&paths::public().join("sfx").join("perso"), ".wav").iter().map(|n| format!("perso-{}", &n[..n.len() - 4])).collect();
    let sounds = sounds.join(" ");
    let sheets: Vec<String> =
        listing(&work.join("style").join("videos"), ".planche.jpg").iter().map(|n| format!("montage/work/style/videos/{n}")).collect();
    let sheets = sheets.join(" ");
    if !has_text && sounds.is_empty() && sheets.is_empty() {
        return fail("Pas de consignes (work/instructions.md) ni d'éléments de style (work/style/).");
    }
    if std::fs::metadata(work.join("segments.json")).map(|m| m.len() == 0).unwrap_or(true) {
        return fail("Rush pas encore transcrit (work/segments.json).");
    }
    if paths::claude().is_none() {
        return fail("Claude Code n'est pas installé.");
    }
    let rush_txt = std::fs::read_to_string(work.join("rush.txt")).unwrap_or_default();
    let rush_txt = rush_txt.trim_end_matches('\n');
    let rush = rush_txt.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("").to_string();
    let langue = language_label(&language_from(&work, "langue_rush.txt")).unwrap_or("Français").to_lowercase();

    let consignes = if has_text {
        "Applique les consignes de montage de montage/work/instructions.md"
    } else {
        "Il n'y a pas de consignes écrites : fais un montage soigné (doublons et ratés retirés) en t'inspirant des éléments de style ci-dessous"
    };
    let mut style = String::new();
    if !sounds.is_empty() {
        style += &format!(
            "\nSons personnels fournis par l'utilisateur (effets utilisables) : {sounds}. Place-les là où ils servent le propos ou imitent le style des exemples, avec \"sfx\": [[segment, mot, \"perso-<nom>\"], …] dans edit_choices.json (le son part au début du mot)."
        );
    }
    let resultat = work.join("style").join("analyse").join("resultat.json");
    if std::fs::metadata(&resultat).map(|m| m.len() > 0).unwrap_or(false) {
        // Analyse détaillée déjà faite (bouton « Analyser les vidéos d'exemple ») : elle sert de guide.
        style += "\nLes vidéos d'exemple ont déjà été analysées : lis montage/work/style/analyse/resultat.json (éléments repérés, reproductibles ou non, style de sous-titres proposé) et applique ce qui est reproductible (coupes, intro, pastilles, sons, et le style de sous-titres proposé dans montage/work/caption_style.json s'il n'existe pas encore).";
    } else if !sheets.is_empty() {
        style += &format!(
            "\nVidéos d'exemple fournies pour le style : regarde leurs planches (12 images chacune, lis-les avec Read) : {sheets}. Repère le rythme des coupes, les textes à l'écran (intro tapée, pastilles), la place et le style des sous-titres, les effets. Reproduis ce que le montage sait faire (coupes, intro, pastilles, caisse, voix modifiée, sons personnels) et écris dans montage/work/analyse_style.md, en quelques lignes, ce que tu as vu et ce qui n'a pas pu être reproduit automatiquement (police, effet visuel particulier…), pour que l'utilisateur le voie."
        );
    }
    let hidden = hidden_effects(&work.join("effets_page.json")).join(", ");
    if !hidden.is_empty() {
        style += &format!(
            "\nEffets écartés par l'utilisateur (page « Rushes et montages ») : {hidden}. Ne les utilise pas, sauf si les consignes les demandent explicitement (cash = caisse, bop = pastille sonore, clavier, voix = voix modifiée, gainvoix = volume de la voix par moment (sans effet sur toi), typed = texte tapé, chip = pastille)."
        );
    }
    let build_doc = described_in("build_edit");
    let fonts_doc = described_in("fonts_lib");
    let prompt = format!(
        "Tu travailles dans le projet Auto-montage (interface locale : ne publie aucune page claude.ai, ne lance aucun rendu).
Le rush « {rush} » vient d'être préparé ; sa transcription par segment est dans montage/work/segments.json. On y parle {langue} : les sous-titres corrigés, l'intro et les pastilles sont dans cette langue, sauf si les consignes en demandent une autre.
{consignes}, comme aux étapes 3 et 5 de CLAUDE.md pour un nouveau rush :{style}
1. Écris montage/work/suggestions.json : {{\"keep\": [ids des segments à garder], \"reasons\": {{\"id\": \"raison du retrait\"}}}} (doublon : garder la 2e prise ; ratés, apartés retirés).
2. Écris montage/work/edit_choices.json au format décrit en tête de {build_doc} (près de CHOICES_PATH), avec \"rush\": \"{rush}\" : coupes au mot près (keep), corrections de sous-titres (fixes), voix modifiée (funny : [segment, premier mot, dernier mot, voix] avec voix = tremblement, robot, ecureuil, lutin, geant, megaphone, telephone, salle, batterie, radio ou choeur), bruit de caisse (cash), pastilles (chips), texte de l'intro (intro), sons (sfx : sons par défaut whoosh, pop, ding, boum, montee, glitch, photo, faux, juste, bop, cash, ou personnels), effets visuels (vfx : zoom avant, zoom sec, dézoom, secousse, flash, mot mis en valeur), selon les consignes, avec mesure (quelques effets bien placés, pas sur chaque phrase). Les réglages de tout le montage (zooms automatiques, sous-titres qui rebondissent, mot prononcé en couleur) vont dans montage/work/habillage.json : {{\"autoZoom\": 0 à 1.6, \"pop\": vrai/faux, \"karaoke\": vrai/faux, \"karaokeColor\": \"#rrggbb\"}}.
3. Si les consignes parlent du style des sous-titres (police, taille, gras, couleur, majuscules, nombre de lignes, taille des lignes entre elles), écris montage/work/caption_style.json au format décrit en tête de {fonts_doc} ; « cd montage && auto-montage fonts_lib » liste les polices utilisables (identifiants) et le style actuel.
4. Vérifie que « cd montage && auto-montage build_edit » passe ; corrige tes fichiers sinon.
Ne modifie aucun autre fichier (sauf montage/work/analyse_style.md, montage/work/caption_style.json et montage/work/habillage.json). Termine par un résumé de deux ou trois lignes."
    );
    run_claude(
        &prompt,
        &[
            "Bash(cd montage && auto-montage build_edit)",
            "Bash(auto-montage build_edit)",
            "Bash(cd montage && auto-montage fonts_lib)",
            "Bash(auto-montage fonts_lib)",
        ],
    )
}

/// `auto-montage style_claude`
pub fn style_claude(_args: &[String]) -> i32 {
    let analyse = paths::work().join("style").join("analyse");
    if std::fs::metadata(analyse.join("faits.json")).map(|m| m.len() == 0).unwrap_or(true) {
        return fail("Mesures absentes : lancer d'abord auto-montage analyze_style.");
    }
    if paths::claude().is_none() {
        return fail("Claude Code n'est pas installé.");
    }
    // work/style/analyse/*/planche-*.jpg, dans l'ordre de « ls ».
    let mut sheets: Vec<String> = vec![];
    for d in util::list_dir(&analyse) {
        if d.starts_with('.') || !analyse.join(&d).is_dir() {
            continue;
        }
        for n in listing(&analyse.join(&d), ".jpg") {
            if n.starts_with("planche-") {
                sheets.push(format!("montage/work/style/analyse/{d}/{n}"));
            }
        }
    }
    sheets.sort();
    let sheets = sheets.join(" ");
    let resultat = analyse.join("resultat.json");
    let _ = std::fs::remove_file(&resultat);

    let prompt = format!(
        "Tu travailles dans le projet Auto-montage (montage automatique de vidéos face caméra, format vertical).
L'utilisateur a fourni des vidéos d'exemple dont il aime le style. Leurs mesures sont dans montage/work/style/analyse/faits.json (durée, instants des coupes, durée moyenne des plans, attaques sonores = bruitages probables, planches). Regarde toutes les planches (Read) : {sheets}
Chaque image porte son instant dans la vidéo (« (coupe) » = juste après un changement de plan).

Repère tout ce qui fait le style : rythme des coupes, sous-titres (police, graisse, taille, couleur, majuscules, contour ou ombre, position, nombre de mots, animation mot par mot…), textes à l'écran (titres, intro, pastilles, listes), zooms et cadrage, transitions, effets visuels (couleurs, filtres, emojis, images ajoutées), sons (bruitages aux instants des attaques sonores, musique).

Ce qu'Auto-montage sait faire :
- coupes au mot près, rythme du montage (retirer blancs et hésitations) ;
- sous-titres sur 2 lignes équilibrées, en bas du visage : police (liste : « cd montage && auto-montage fonts_lib »), graisse, taille (36-120 px sur 1080), majuscules, couleur, ombre (intensité 0-100, diffusion du flou 0-60 px, décalage ox vers la droite et oy vers le bas en px, 40 px au plus) ; apparition avec rebond, mot prononcé en couleur (karaoké), mots mis en valeur (plus gros, en couleur) ; pas de contour, position fixe ;
- intro en texte tapé au clavier, pastilles (bulle blanche avec texte, son « bop ») ;
- zooms automatiques alternés sur le visage (aucun, légers, normaux, marqués), et sur un moment : zoom avant, zoom sec, dézoom, secousse, flash blanc ;
- sons fournis : caisse enregistreuse, bop, clavier, whoosh, pop, ding, boum, montée, glitch, déclencheur photo, faux (buzzer), juste (carillon) ; sons personnels ajoutés par l'utilisateur ; voix modifiée (tremblante) ;
- pas de transitions élaborées (glissés, volets), filtres de couleur, emojis, images ajoutées, musique de fond.

Écris montage/work/style/analyse/resultat.json, en français, exactement sous cette forme (JSON valide) :
{{\"resume\": \"2 ou 3 phrases sur le style\",
 \"rythme\": \"le rythme mesuré (un plan toutes les X s, coupes…), en mots simples\",
 \"elements\": [{{\"categorie\": \"Sous-titres|Rythme|Texte à l'écran|Zoom et cadrage|Transitions|Effets visuels|Sons\", \"observe\": \"ce que tu vois, précis\", \"ou\": \"vidéo et instants\", \"reproductible\": \"oui|en partie|non\", \"comment\": \"le réglage d'Auto-montage à utiliser, ou ce qui manque et ce que l'utilisateur peut faire (ex. ajouter un son « whoosh » comme son personnel)\"}}],
 \"sousTitres\": null ou {{\"font\": \"identifiant de police de fonts_lib\", \"weight\": 700, \"size\": 64, \"uppercase\": true, \"color\": \"#rrggbb\", \"shadow\": 50, \"blur\": 14, \"ox\": 0, \"oy\": 0, \"lines\": 2, \"lineSizes\": [100, 100, 100], \"posX\": 0, \"posY\": 0}} (le style de sous-titres le plus proche possible des exemples ; lines : lignes de chaque sous-titre, de 1 à 6 (une seule pour un sous-titre court) ; lineSizes : taille de chaque ligne en % de la première, de 50 à 200 ; posX, posY : décalage depuis la place d'origine (sous le milieu de l'image), vers la droite et vers le bas, en % de la moitié de l'image, dans un cercle de rayon 100),
 \"aFaire\": [\"conseils concrets à l'utilisateur pour s'en approcher\"]}}
Puis écris montage/work/analyse_style.md : quelques lignes, le résumé et ce qui ne peut pas être reproduit automatiquement.
L'utilisateur n'est pas développeur : dans « comment » et « aFaire », parle des boutons de l'interface (« Ajouter des effets sonores… » ou « Ajouter des sons… » pour un bruitage, « Style des sous-titres » sur la page des moments, les consignes de montage pour le rythme, l'intro et les pastilles), jamais de chemins de fichiers.
Ne modifie aucun autre fichier. Termine par une phrase de résumé."
    );
    let code = run_claude(
        &prompt,
        &[
            "Bash(cd montage && auto-montage fonts_lib)",
            "Bash(auto-montage fonts_lib)",
        ],
    );
    if code != 0 {
        return code;
    }
    match check_result(&resultat) {
        Ok(n) => {
            crate::progress::say(&format!("Analyse écrite : {n} éléments repérés"));
            0
        }
        Err(e) => fail(&format!("Claude n'a pas écrit une analyse lisible (work/style/analyse/resultat.json) : {e}")),
    }
}

/// Nombre d'éléments de l'analyse écrite par Claude (objet JSON avec une liste « elements »).
fn check_result(path: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    match data.get("elements") {
        Some(Value::Array(a)) => Ok(a.len()),
        _ => Err("pas de liste « elements »".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_stay_in_project() {
        let roots = rule_roots();
        assert_eq!(roots[0], "montage");
        // forme absolue « //chemin » de Claude Code (Unix : « //home/… »)
        assert!(roots.iter().skip(1).all(|r| r.starts_with("//") && !r.ends_with('/')));
        let w = write_rules();
        assert!(w.contains(&"Edit(montage/work/suggestions.json)".to_string()));
        assert!(w.iter().all(|r| r.contains("/work/") && !r.contains("work/**)")));
        assert!(read_rules().iter().all(|r| r.ends_with("/**)")));
    }

    #[test]
    fn result_check() {
        let dir = std::env::temp_dir().join(format!("am-claude-test-{}", std::process::id()));
        util::mkdirs(&dir);
        let p = dir.join("r.json");
        std::fs::write(&p, r#"{"elements": [{}, {}]}"#).unwrap();
        assert_eq!(check_result(&p), Ok(2));
        std::fs::write(&p, r#"{"elements": 3}"#).unwrap();
        assert!(check_result(&p).is_err());
        std::fs::write(&p, "pas du json").unwrap();
        assert!(check_result(&p).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
