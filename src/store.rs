//! Petit stockage clé / valeur persistant dans `/data`.
//!
//! Fichier texte `clé=valeur`, une entrée par ligne, écrit de façon atomique (fichier temporaire + renommage).
//! Les clés ne doivent contenir ni `=` ni saut de ligne (remplacés par `_`).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct Store {
    path: PathBuf,
    map: BTreeMap<String, String>,
}

fn clean_key(k: &str) -> String {
    k.replace(['=', '\n', '\r'], "_")
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r")
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

impl Store {
    /// Ouvre (ou crée) `<data_dir>/<name>`.
    pub fn open(name: &str) -> Store {
        Store::open_at(crate::sys::data_dir().join(name))
    }

    pub fn open_at(path: impl AsRef<Path>) -> Store {
        let path = path.as_ref().to_path_buf();
        let mut map = BTreeMap::new();
        if let Ok(txt) = fs::read_to_string(&path) {
            for line in txt.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    map.insert(k.to_string(), unescape(v));
                }
            }
        }
        Store { path, map }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(&clean_key(key)).map(|s| s.as_str())
    }

    pub fn get_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_string()
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse().ok()
    }

    pub fn get_f32(&self, key: &str) -> Option<f32> {
        self.get(key)?.trim().parse().ok()
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.trim() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(|k| k.as_str())
    }

    /// Enregistre `value` sous `key` et sauvegarde immédiatement.
    pub fn set(&mut self, key: &str, value: impl ToString) -> io::Result<()> {
        self.map.insert(clean_key(key), value.to_string());
        self.save()
    }

    pub fn remove(&mut self, key: &str) -> io::Result<()> {
        self.map.remove(&clean_key(key));
        self.save()
    }

    pub fn save(&self) -> io::Result<()> {
        let mut txt = String::new();
        for (k, v) in &self.map {
            txt.push_str(k);
            txt.push('=');
            txt.push_str(&escape(v));
            txt.push('\n');
        }
        if let Some(dir) = self.path.parent() {
            if !dir.as_os_str().is_empty() {
                fs::create_dir_all(dir)?;
            }
        }
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, txt)?;
        fs::rename(&tmp, &self.path)
    }
}