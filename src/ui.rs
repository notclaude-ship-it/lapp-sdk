//! Couleurs, thème et petits composants d'interface (boutons, barre de titre, lignes de liste, interrupteur).
//!
//! Tout est dessiné avec les primitives de `Gfx` : rien n'est retenu entre deux images, c'est à l'app
//! de mémoriser l'état (bouton pressé, ligne sélectionnée…) et de tester les zones avec `Rect::contains`.

use tiny_skia::Color;

use crate::gfx::Gfx;
use crate::runtime::Ctx;
use crate::text::Text;

/// Couleur opaque depuis `0xRRGGBB`.
pub fn rgb(hex: u32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

/// Couleur depuis `0xRRGGBB` et une opacité de 0 à 1.
pub fn rgba(hex: u32, a: f32) -> Color {
    with_alpha(rgb(hex), a)
}

/// Même couleur avec une autre opacité.
pub fn with_alpha(mut c: Color, a: f32) -> Color {
    c.set_alpha(a.clamp(0.0, 1.0));
    c
}

/// Éclaircit (`f` > 1) ou assombrit (`f` < 1) une couleur.
pub fn shade(c: Color, f: f32) -> Color {
    let k = |v: f32| (v * f).clamp(0.0, 1.0);
    Color::from_rgba(k(c.red()), k(c.green()), k(c.blue()), c.alpha()).unwrap_or(c)
}

/// Palette de l'interface.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub bg: Color,
    pub surface: Color,
    pub text: Color,
    pub subtext: Color,
    pub accent: Color,
    pub danger: Color,
    pub line: Color,
    pub press: Color,
}

impl Theme {
    pub fn light() -> Theme {
        Theme {
            bg: rgb(0xF7F7FB),
            surface: rgb(0xFFFFFF),
            text: rgb(0x1C1C21),
            subtext: rgb(0x8C8C99),
            accent: rgb(0x007AFF),
            danger: rgb(0xFF3B30),
            line: rgb(0xCCCCD4),
            press: rgb(0xD6E0F2),
        }
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::light()
    }
}

/// Rectangle (x, y, largeur, hauteur) pour placer les composants et tester les appuis.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    /// Rétrécit de `d` de chaque côté.
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }
    /// Découpe une bande de hauteur `h` en haut : (bande, reste).
    pub fn split_top(&self, h: f32) -> (Rect, Rect) {
        let h = h.min(self.h);
        (Rect::new(self.x, self.y, self.w, h), Rect::new(self.x, self.y + h, self.w, self.h - h))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Fond de la couleur d'accent, texte blanc.
    Primary,
    /// Fond blanc, texte de la couleur d'accent.
    Secondary,
    /// Fond rouge, texte blanc.
    Danger,
}

/// Bouton arrondi avec ombre légère ; `pressed` l'assombrit.
pub fn button(ctx: &Ctx, g: &mut Gfx, r: Rect, label: &str, style: ButtonStyle, pressed: bool) {
    let t = ctx.theme;
    let s = ctx.scale();
    let (base, fg) = match style {
        ButtonStyle::Primary => (t.accent, t.surface),
        ButtonStyle::Secondary => (t.surface, t.accent),
        ButtonStyle::Danger => (t.danger, t.surface),
    };
    let fill = if pressed { shade(base, 0.85) } else { base };
    let radius = r.h * 0.28;
    g.rrect(r.x, r.y + 1.5 * s, r.w, r.h, radius, rgba(0x000000, 0.14));
    g.rrect(r.x, r.y, r.w, r.h, radius, fill);
    let st = Text::new(r.h * 0.42).bold().color(fg).center().max_width(r.w - 12.0 * s);
    let (cx, cy) = r.center();
    ctx.text_mid(g, label, cx, cy, &st);
}

/// Résultat de `header` : hauteur de la barre et zone du bouton « retour ».
pub struct Header {
    pub height: f32,
    pub back: Option<Rect>,
}

/// Barre de titre en haut de l'écran ; `back` = libellé du bouton retour (« Retour », « Accueil »…).
pub fn header(ctx: &Ctx, g: &mut Gfx, title: &str, back: Option<&str>) -> Header {
    let t = ctx.theme;
    let s = ctx.scale();
    let (w, _) = ctx.size();
    let w = w as f32;
    let h = 44.0 * s;
    g.rect(0.0, 0.0, w, h, t.surface);
    g.rect(0.0, h - 1.0, w, 1.0, t.line);
    let st = Text::new(17.0 * s).bold().color(t.text).center().max_width(w * 0.56);
    ctx.text_mid(g, title, w / 2.0, h / 2.0, &st);
    let mut area = None;
    if let Some(label) = back {
        let st = Text::new(15.0 * s).color(t.accent).left().max_width(w * 0.3);
        ctx.text_mid(g, &format!("‹ {label}"), 10.0 * s, h / 2.0, &st);
        area = Some(Rect::new(0.0, 0.0, w * 0.32, h));
    }
    Header { height: h, back: area }
}

/// Ligne de liste : titre à gauche, détail gris à droite, filet en bas. `pressed` = fond de sélection.
pub fn list_row(ctx: &Ctx, g: &mut Gfx, r: Rect, title: &str, detail: Option<&str>, pressed: bool) {
    let t = ctx.theme;
    let s = ctx.scale();
    g.rect(r.x, r.y, r.w, r.h, if pressed { t.press } else { t.surface });
    g.rect(r.x + 12.0 * s, r.bottom() - 1.0, (r.w - 12.0 * s).max(0.0), 1.0, t.line);
    let mut title_w = r.w - 24.0 * s;
    if let Some(d) = detail {
        let dst = Text::new(13.0 * s).color(t.subtext).right().max_width(r.w * 0.4);
        ctx.text_mid(g, d, r.right() - 12.0 * s, r.y + r.h / 2.0, &dst);
        title_w = r.w * 0.55;
    }
    let st = Text::new(16.0 * s).color(t.text).left().max_width(title_w);
    ctx.text_mid(g, title, r.x + 12.0 * s, r.y + r.h / 2.0, &st);
}

/// Interrupteur à bascule (dessin seul : le changement d'état est à la charge de l'app).
pub fn toggle(ctx: &Ctx, g: &mut Gfx, r: Rect, on: bool) {
    let t = ctx.theme;
    g.rrect(r.x, r.y, r.w, r.h, r.h / 2.0, if on { t.accent } else { t.line });
    let k = r.h - 4.0;
    let kx = if on { r.right() - 2.0 - k } else { r.x + 2.0 };
    g.rrect(kx, r.y + 2.0, k, k, k / 2.0, t.surface);
}