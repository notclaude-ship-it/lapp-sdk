//! Événements d'entrée (tactile et souris unifiés) et défilement avec inertie.

/// Touche clavier (Echap est gérée à part : voir `App::on_back`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Back,
    Enter,
    Space,
    Up,
    Down,
    Left,
    Right,
    Char(char),
}

/// Événement transmis à `App::event`. Coordonnées en pixels, origine en haut à gauche.
///
/// Un geste produit : `Down`, des `Move` (+ `Drag` une fois le seuil de glissement dépassé), `Up`,
/// puis soit `Tap` (pas de glissement), soit `DragEnd` (avec la vitesse du doigt).
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Down { x: f32, y: f32 },
    Move { x: f32, y: f32 },
    Up { x: f32, y: f32 },
    /// Appui bref sans glissement : l'événement à utiliser pour les boutons.
    Tap { x: f32, y: f32 },
    /// Glissement en cours ; `dx`/`dy` = déplacement depuis l'événement précédent.
    Drag { x: f32, y: f32, dx: f32, dy: f32 },
    /// Fin d'un glissement ; `vx`/`vy` = vitesse du doigt en px/s (0 si le doigt s'est arrêté).
    DragEnd { x: f32, y: f32, vx: f32, vy: f32 },
    /// Molette / pavé tactile ; même convention de signe que `Drag::dy`.
    Wheel { dy: f32 },
    Key(Key),
}

/// Défilement vertical avec inertie.
///
/// ```ignore
/// // dans event() :
/// Event::Drag { dy, .. } | Event::Wheel { dy } => self.scroll.drag(dy),
/// Event::DragEnd { vy, .. } => self.scroll.fling(vy),
/// Event::Down { .. } => self.scroll.stop(),
/// // dans tick() :
/// if self.scroll.update(dt) { ctx.animate(); }
/// // dans render() : self.scroll.set_content(hauteur_contenu, hauteur_visible); puis y = -self.scroll.pos
/// ```
#[derive(Clone, Debug, Default)]
pub struct Scroller {
    /// Décalage courant (0 = haut du contenu).
    pub pos: f32,
    /// Décalage maximal.
    pub max: f32,
    vel: f32,
}

impl Scroller {
    pub fn new() -> Scroller {
        Scroller::default()
    }

    /// Indique la hauteur du contenu et celle de la zone visible (recale `pos` si besoin).
    pub fn set_content(&mut self, content_h: f32, view_h: f32) {
        self.max = (content_h - view_h).max(0.0);
        self.pos = self.pos.clamp(0.0, self.max);
    }

    /// Suit le doigt (`dy` = déplacement du doigt).
    pub fn drag(&mut self, dy: f32) {
        self.vel = 0.0;
        self.pos = (self.pos - dy).clamp(0.0, self.max);
    }

    /// Lance l'inertie à la fin d'un glissement (`vy` = vitesse du doigt, px/s).
    pub fn fling(&mut self, vy: f32) {
        self.vel = if vy.abs() > 120.0 { -vy } else { 0.0 };
    }

    pub fn stop(&mut self) {
        self.vel = 0.0;
    }

    pub fn jump_to(&mut self, pos: f32) {
        self.vel = 0.0;
        self.pos = pos.clamp(0.0, self.max);
    }

    /// Fait avancer l'inertie ; renvoie `true` tant qu'il faut redessiner.
    pub fn update(&mut self, dt: f32) -> bool {
        if self.vel.abs() <= 15.0 {
            self.vel = 0.0;
            return false;
        }
        let before = self.pos;
        self.pos = (self.pos + self.vel * dt).clamp(0.0, self.max);
        if (self.pos - before).abs() < 0.01 {
            self.vel = 0.0;
            return false;
        }
        self.vel *= (-dt * 3.2).exp();
        true
    }
}