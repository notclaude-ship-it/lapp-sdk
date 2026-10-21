//! Accès à l'environnement d'exécution d'une app lapp (bac à sable bubblewrap).
//!
//! Dans le bac à sable : l'image de l'app est sur `/app` (lecture seule), les données persistantes
//! sur `/data` (aussi `$HOME`). Hors bac à sable (développement sur PC), les variables
//! `LAPP_DATA` et `LAPP_APP` redéfinissent ces deux dossiers.

use std::fs;
use std::path::{Path, PathBuf};

use tiny_skia::Pixmap;

/// Dossier de données persistantes de l'app (créé si besoin).
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LAPP_DATA") {
        let p = PathBuf::from(d);
        let _ = fs::create_dir_all(&p);
        return p;
    }
    if Path::new("/data").is_dir() {
        return PathBuf::from("/data");
    }
    let p = PathBuf::from(".lapp-data");
    let _ = fs::create_dir_all(&p);
    p
}

/// Dossier de l'image de l'app (ses ressources, en lecture seule).
pub fn app_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LAPP_APP") {
        return PathBuf::from(d);
    }
    if Path::new("/app").is_dir() {
        return PathBuf::from("/app");
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Chemin d'une ressource de l'app (`assets/fond.png` -> `/app/assets/fond.png`).
pub fn app_path(rel: &str) -> PathBuf {
    if rel.starts_with('/') {
        PathBuf::from(rel)
    } else {
        app_dir().join(rel)
    }
}

/// Charge une image PNG de l'app (chemin relatif à `app_dir()`).
pub fn load_png(rel: &str) -> Option<Pixmap> {
    Pixmap::load_png(app_path(rel)).ok()
}

/// Vrai quand l'app tourne dans le bac à sable lapp.
pub fn in_sandbox() -> bool {
    Path::new("/app").is_dir() && Path::new("/data").is_dir() && std::env::var("LAPP_APP").is_err()
}

/// Chemin du binaire `lapp` (permission `install` : monté sur `/lapp`).
pub fn lapp_bin() -> PathBuf {
    if Path::new("/lapp").is_file() {
        PathBuf::from("/lapp")
    } else {
        PathBuf::from("lapp")
    }
}

/// Valeur de l'option `--clé valeur` de la ligne de commande.
pub fn arg_value(key: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
}

/// Échelle d'interface imposée (dpi commun à lapp-home et aux apps) : `--scale X`, sinon la variable
/// `LAPP_SCALE`, que lapp-home exporte à chaque app qu'il lance. `None` : l'app calcule son échelle toute seule.
pub fn scale_override() -> Option<f32> {
    let parse = |s: &str| s.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).map(|v| v.clamp(0.5, 8.0));
    arg_value("--scale")
        .as_deref()
        .and_then(parse)
        .or_else(|| std::env::var("LAPP_SCALE").ok().as_deref().and_then(parse))
}

/// Vrai si l'option `--drapeau` est présente.
pub fn arg_flag(key: &str) -> bool {
    std::env::args().skip(1).any(|a| a == key)
}