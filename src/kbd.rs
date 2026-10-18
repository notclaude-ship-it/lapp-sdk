//! Client du clavier virtuel de lapp-home.
//!
//! Protocole (X11) : l'app pose sur sa fenêtre de premier niveau la propriété `_LAPP_KEYBOARD` (CARDINAL 32 bits) :
//! 1 = clavier lettres, 2 = clavier chiffres/symboles, 0 = pas de clavier. Re-écrire une valeur non nulle réaffiche
//! le clavier après qu'on l'a masqué. Tant qu'il est visible, lapp-home écrit sa hauteur (px) dans
//! `_LAPP_KEYBOARD_HEIGHT` sur la même fenêtre. Les touches arrivent ensuite comme des événements `Event::Key`.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, InputFocus, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub(crate) struct KbdClient {
    conn: RustConnection,
    a_kbd: Atom,
    a_h: Atom,
}

impl KbdClient {
    pub fn connect() -> Option<KbdClient> {
        let (conn, _) = RustConnection::connect(None).ok()?;
        let a_kbd = conn.intern_atom(false, b"_LAPP_KEYBOARD").ok()?.reply().ok()?.atom;
        let a_h = conn.intern_atom(false, b"_LAPP_KEYBOARD_HEIGHT").ok()?.reply().ok()?.atom;
        Some(KbdClient { conn, a_kbd, a_h })
    }

    pub fn set(&self, win: u32, mode: u32) {
        let _ = self.conn.change_property32(PropMode::REPLACE, win, self.a_kbd, AtomEnum::CARDINAL, &[mode]);
        if mode != 0 {
            // Sans gestionnaire de fenêtres personne ne donne le focus d'entrée X à l'app : les touches du clavier
            // virtuel (XTEST, envoyées à la fenêtre qui a le focus) partiraient vers la racine. On le prend nous-mêmes.
            match self.conn.set_input_focus(InputFocus::PARENT, win, x11rb::CURRENT_TIME) {
                Ok(c) => {
                    if let Err(e) = c.check() {
                        eprintln!("clavier : set_input_focus refusé : {e}");
                    }
                }
                Err(e) => eprintln!("clavier : set_input_focus impossible : {e}"),
            }
        }
        let _ = self.conn.flush();
    }

    /// Hauteur du clavier en pixels (0 s'il est masqué ou si lapp-home ne le gère pas).
    pub fn height(&self, win: u32) -> u32 {
        self.conn
            .get_property(false, win, self.a_h, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
            .unwrap_or(0)
    }
}