//! Sortie audio (permission `audio` du manifeste).
//!
//! Deux niveaux :
//!
//! * [`AudioOut`] : flux PCM S16 (little-endian) envoyé au serveur PulseAudio / PipeWire de l'hôte via
//!   `libpulse-simple`, chargée à l'exécution (`dlopen`) : aucune dépendance de compilation, et si la
//!   bibliothèque ou le serveur est absent l'app continue sans son.
//! * [`play_file`] : lance un lecteur système (`pw-play`, `paplay`, `aplay`…) sur un fichier son.
//!
//! ```no_run
//! use lapp_sdk::audio::AudioOut;
//!
//! let mut out = AudioOut::new("Mon app", 48_000, 2);
//! let silence = vec![0i16; 48_000 * 2]; // 1 s stéréo
//! out.write(&silence);
//! out.drain();
//! ```

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

extern "C" {
    fn dlopen(file: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;
const PA_SAMPLE_S16LE: c_int = 3;
const PA_STREAM_PLAYBACK: c_int = 1;

#[repr(C)]
struct PaSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct PaAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

type NewFn = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    *const c_char,
    *const PaSpec,
    *const c_void,
    *const PaAttr,
    *mut c_int,
) -> *mut c_void;
type WriteFn = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;
type CtlFn = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;
type FreeFn = unsafe extern "C" fn(*mut c_void);

struct PaApi {
    new: NewFn,
    write: WriteFn,
    flush: CtlFn,
    drain: CtlFn,
    free: FreeFn,
}

fn api() -> Option<&'static PaApi> {
    static API: OnceLock<Option<PaApi>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let mut h = dlopen(b"libpulse-simple.so.0\0".as_ptr() as *const c_char, RTLD_NOW);
        if h.is_null() {
            h = dlopen(b"libpulse-simple.so\0".as_ptr() as *const c_char, RTLD_NOW);
        }
        if h.is_null() {
            eprintln!("lapp-sdk audio : libpulse-simple introuvable, pas de son");
            return None;
        }
        let sym = |n: &[u8]| dlsym(h, n.as_ptr() as *const c_char);
        let p = [
            sym(b"pa_simple_new\0"),
            sym(b"pa_simple_write\0"),
            sym(b"pa_simple_flush\0"),
            sym(b"pa_simple_drain\0"),
            sym(b"pa_simple_free\0"),
        ];
        if p.iter().any(|x| x.is_null()) {
            eprintln!("lapp-sdk audio : symboles pulse-simple manquants");
            return None;
        }
        Some(PaApi {
            new: std::mem::transmute::<*mut c_void, NewFn>(p[0]),
            write: std::mem::transmute::<*mut c_void, WriteFn>(p[1]),
            flush: std::mem::transmute::<*mut c_void, CtlFn>(p[2]),
            drain: std::mem::transmute::<*mut c_void, CtlFn>(p[3]),
            free: std::mem::transmute::<*mut c_void, FreeFn>(p[4]),
        })
    })
    .as_ref()
}

/// Flux de lecture PCM 16 bits entrelacé vers le serveur audio de l'hôte.
///
/// La connexion est établie au premier [`write`](AudioOut::write) (et retentée une seule fois si
/// elle échoue : ensuite le flux reste muet, sans bloquer l'app). Peut être déplacé dans un autre
/// thread (utile depuis un callback de décodeur).
pub struct AudioOut {
    name: CString,
    rate: u32,
    channels: u8,
    latency_ms: u32,
    pa: *mut c_void,
    failed: bool,
}

// Le pointeur pa_simple n'est utilisé que depuis un seul thread à la fois (méthodes `&mut self`).
unsafe impl Send for AudioOut {}

impl AudioOut {
    /// `name` : nom du flux affiché dans le mixeur ; `rate` en Hz (ex. 48000) ; `channels` : 1 ou 2.
    pub fn new(name: &str, rate: u32, channels: u8) -> AudioOut {
        AudioOut {
            name: CString::new(name.replace('\0', "")).unwrap_or_default(),
            rate: rate.max(1),
            channels: channels.max(1),
            latency_ms: 150,
            pa: std::ptr::null_mut(),
            failed: false,
        }
    }

    /// Latence cible en millisecondes (150 par défaut). À régler avant le premier `write`.
    pub fn latency_ms(mut self, ms: u32) -> AudioOut {
        self.latency_ms = ms.max(10);
        self
    }

    /// `true` si le serveur audio a pu être joint (après un premier `write`).
    pub fn is_open(&self) -> bool {
        !self.pa.is_null()
    }

    fn connect(&mut self) -> bool {
        if !self.pa.is_null() {
            return true;
        }
        if self.failed {
            return false;
        }
        let Some(api) = api() else {
            self.failed = true;
            return false;
        };
        let spec = PaSpec { format: PA_SAMPLE_S16LE, rate: self.rate, channels: self.channels };
        let bytes_per_s = self.rate * self.channels as u32 * 2;
        let attr = PaAttr {
            maxlength: u32::MAX,
            tlength: bytes_per_s / 1000 * self.latency_ms,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: u32::MAX,
        };
        let mut err: c_int = 0;
        let stream: &CStr = self.name.as_c_str();
        let pa = unsafe {
            (api.new)(
                std::ptr::null(),
                stream.as_ptr(),
                PA_STREAM_PLAYBACK,
                std::ptr::null(),
                b"lecture\0".as_ptr() as *const c_char,
                &spec,
                std::ptr::null(),
                &attr,
                &mut err,
            )
        };
        if pa.is_null() {
            eprintln!("lapp-sdk audio : sortie indisponible (pulse : {err})");
            self.failed = true;
            return false;
        }
        self.pa = pa;
        true
    }

    /// Envoie des échantillons S16 entrelacés ; bloque tant que le tampon du serveur est plein.
    /// Renvoie `false` si la sortie est indisponible.
    pub fn write(&mut self, samples: &[i16]) -> bool {
        if samples.is_empty() {
            return true;
        }
        // SAFETY: une tranche d'i16 est lisible comme octets ; l'ordre natif est little-endian sur
        // x86_64 et aarch64, ce qu'attend PA_SAMPLE_S16LE.
        let bytes = unsafe { std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 2) };
        self.write_bytes(bytes)
    }

    /// Variante sur octets bruts (S16LE entrelacé), pour les callbacks de décodeurs C.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return true;
        }
        if !self.connect() {
            return false;
        }
        let Some(api) = api() else { return false };
        let mut err: c_int = 0;
        unsafe { (api.write)(self.pa, bytes.as_ptr() as *const c_void, bytes.len(), &mut err) >= 0 }
    }

    /// Jette l'audio en attente (changement de position, arrêt).
    pub fn flush(&mut self) {
        if let (Some(api), false) = (api(), self.pa.is_null()) {
            let mut err: c_int = 0;
            unsafe { (api.flush)(self.pa, &mut err) };
        }
    }

    /// Attend la fin de la lecture de l'audio déjà envoyé.
    pub fn drain(&mut self) {
        if let (Some(api), false) = (api(), self.pa.is_null()) {
            let mut err: c_int = 0;
            unsafe { (api.drain)(self.pa, &mut err) };
        }
    }
}

impl Drop for AudioOut {
    fn drop(&mut self) {
        if let (Some(api), false) = (api(), self.pa.is_null()) {
            unsafe { (api.free)(self.pa) };
            self.pa = std::ptr::null_mut();
        }
    }
}

/// Lance un lecteur système sur un fichier son et renvoie le processus (`kill()` pour arrêter).
///
/// Essaie `aplay` (WAV uniquement en premier), `pw-play`, `paplay`, `mpv`, `play`, `ffplay`.
/// Renvoie `None` si aucun n'est disponible.
pub fn play_file(path: &Path) -> Option<Child> {
    let wav = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    let mut players: Vec<(&str, &[&str])> = vec![
        ("pw-play", &[]),
        ("paplay", &[]),
        ("mpv", &["--no-video", "--really-quiet"]),
        ("play", &["-q"]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
        ("aplay", &["-q"]),
    ];
    if wav {
        players.insert(0, ("aplay", &["-q"]));
    }
    for (cmd, args) in players {
        if let Ok(child) = Command::new(cmd).args(args).arg(path).stdin(Stdio::null()).spawn() {
            return Some(child);
        }
    }
    None
}