//! Texte : chargement des polices système et rastérisation.
//!
//! Le texte est rastérisé une fois dans une petite image (mise en cache côté GPU/CPU par `Ctx::text`),
//! puis recopiée à chaque image : dessiner du texte coûte presque rien une fois en cache.

use std::fs;

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use tiny_skia::{Color, Pixmap};

/// Famille de police.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Regular,
    Bold,
    Mono,
}

/// Alignement horizontal par rapport à la coordonnée `x` passée à `Ctx::text`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Style de texte (construction par chaînage : `Text::new(14.0).bold().center()`).
#[derive(Clone, Copy, Debug)]
pub struct Text {
    /// Hauteur d'em en pixels.
    pub px: f32,
    pub kind: Kind,
    pub color: Color,
    pub align: Align,
    /// Largeur maximale : le texte est raccourci avec une ellipse au-delà.
    pub max_w: Option<f32>,
    /// Ellipse au début plutôt qu'à la fin (utile pour les chemins).
    pub ellipsis_left: bool,
}

impl Text {
    pub fn new(px: f32) -> Text {
        Text { px, kind: Kind::Regular, color: Color::BLACK, align: Align::Left, max_w: None, ellipsis_left: false }
    }
    pub fn bold(mut self) -> Self {
        self.kind = Kind::Bold;
        self
    }
    pub fn mono(mut self) -> Self {
        self.kind = Kind::Mono;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    pub fn left(mut self) -> Self {
        self.align = Align::Left;
        self
    }
    pub fn center(mut self) -> Self {
        self.align = Align::Center;
        self
    }
    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_w = Some(w);
        self
    }
    pub fn ellipsis_left(mut self) -> Self {
        self.ellipsis_left = true;
        self
    }

    pub(crate) fn key(&self, s: &str) -> String {
        let c = self.color.to_color_u8();
        format!(
            "t{}|{:.1}|{:02x}{:02x}{:02x}{:02x}|{}|{}|{}",
            self.kind as u8,
            self.px,
            c.red(),
            c.green(),
            c.blue(),
            c.alpha(),
            self.max_w.map(|m| m as i32).unwrap_or(-1),
            self.ellipsis_left as u8,
            s
        )
    }
}

/// Polices chargées (Liberation / DejaVu ; `LAPP_FONT` force la police normale).
pub struct Fonts {
    reg: Option<FontVec>,
    bold: Option<FontVec>,
    mono: Option<FontVec>,
}

fn first_font(paths: &[&str]) -> Option<FontVec> {
    paths.iter().find_map(|p| FontVec::try_from_vec(fs::read(p).ok()?).ok())
}

impl Fonts {
    pub fn load() -> Fonts {
        let mut reg = std::env::var("LAPP_FONT").ok().and_then(|p| first_font(&[p.as_str()]));
        if reg.is_none() {
            reg = first_font(&[
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
                "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
            ]);
        }
        let bold = first_font(&[
            "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        ]);
        let mono = first_font(&[
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        ]);
        if reg.is_none() {
            eprintln!("lapp-sdk : aucune police trouvée, texte désactivé");
        }
        Fonts { reg, bold, mono }
    }

    fn get(&self, k: Kind) -> Option<&FontVec> {
        match k {
            Kind::Regular => self.reg.as_ref(),
            Kind::Bold => self.bold.as_ref().or(self.reg.as_ref()),
            Kind::Mono => self.mono.as_ref().or(self.reg.as_ref()),
        }
    }
}

fn em_scale(font: &FontVec, px: f32) -> PxScale {
    PxScale::from(px * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0))
}

fn text_width(font: &FontVec, scale: PxScale, s: &str) -> f32 {
    let sf = font.as_scaled(scale);
    let mut w = 0.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

/// Raccourcit `s` avec une ellipse pour tenir dans `max` pixels (ellipse au début si `left`).
fn fit(font: &FontVec, scale: PxScale, s: &str, max: f32, left: bool) -> String {
    if text_width(font, scale, s) <= max {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        if left {
            chars.remove(0);
        } else {
            chars.pop();
        }
        let body: String = chars.iter().collect();
        let t = if left { format!("…{body}") } else { format!("{body}…") };
        if text_width(font, scale, &t) <= max {
            return t;
        }
    }
    String::from("…")
}

/// Largeur (px) de `s` dans le style `st` (en tenant compte de `max_w`).
pub(crate) fn measure(fonts: &Fonts, s: &str, st: &Text) -> f32 {
    let Some(font) = fonts.get(st.kind) else { return 0.0 };
    let scale = em_scale(font, st.px);
    match st.max_w {
        Some(m) => text_width(font, scale, &fit(font, scale, s, m, st.ellipsis_left)),
        None => text_width(font, scale, s),
    }
}

/// Hauteur d'une ligne (px) pour le style `st`.
pub(crate) fn line_height(fonts: &Fonts, st: &Text) -> f32 {
    match fonts.get(st.kind) {
        Some(font) => {
            let sf = font.as_scaled(em_scale(font, st.px));
            sf.ascent() - sf.descent()
        }
        None => st.px * 1.17,
    }
}

/// Rastérise `s` dans une petite image transparente (marge de 1 px, prémultipliée).
pub(crate) fn render(fonts: &Fonts, s: &str, st: &Text) -> Pixmap {
    let Some(font) = fonts.get(st.kind) else { return Pixmap::new(1, 1).unwrap() };
    let scale = em_scale(font, st.px);
    let owned;
    let s = match st.max_w {
        Some(m) => {
            owned = fit(font, scale, s, m, st.ellipsis_left);
            owned.as_str()
        }
        None => s,
    };
    let sf = font.as_scaled(scale);
    let tw = text_width(font, scale, s);
    let w = (tw.ceil() as u32 + 3).clamp(1, 4096);
    let h = ((sf.ascent() - sf.descent()).ceil() as u32 + 3).clamp(1, 512);
    let mut pm = Pixmap::new(w, h).unwrap();
    let (pw, ph) = (w as i32, h as i32);
    let baseline = 1.0 + sf.ascent();
    let rgb = [st.color.red(), st.color.green(), st.color.blue()];
    let ca = st.color.alpha();
    let data = pm.data_mut();
    let mut x = 1.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(scale, point(x, baseline));
        x += sf.h_advance(id);
        prev = Some(id);
        if let Some(og) = font.outline_glyph(g) {
            let b = og.px_bounds();
            let (bx, by) = (b.min.x as i32, b.min.y as i32);
            og.draw(|gx, gy, cov| {
                let (px_, py_) = (bx + gx as i32, by + gy as i32);
                if px_ < 0 || py_ < 0 || px_ >= pw || py_ >= ph {
                    return;
                }
                let a = (cov * ca).clamp(0.0, 1.0);
                let i = ((py_ * pw + px_) * 4) as usize;
                for c in 0..3 {
                    data[i + c] = (data[i + c] as f32 * (1.0 - a) + rgb[c] * 255.0 * a) as u8;
                }
                data[i + 3] = (data[i + 3] as f32 * (1.0 - a) + 255.0 * a) as u8;
            });
        }
    }
    pm
}