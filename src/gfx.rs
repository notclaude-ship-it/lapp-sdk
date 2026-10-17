//! Couche de dessin : OpenGL ES (GPU, EGL) avec repli CPU (tiny-skia).
//!
//! Primitives : rectangle, rectangle arrondi (un shader, distance signée + anti-crénelage) et images
//! (texte, icônes) envoyées une seule fois en texture puis réutilisées d'une image à l'autre.

use std::collections::HashMap;

use glow::HasContext;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

const VS: &str = "attribute vec2 p;
uniform vec4 r;
uniform vec2 s;
varying vec2 uv;
varying vec2 lp;
void main() {
    uv = p;
    lp = (p - vec2(0.5)) * r.zw;
    vec2 px = r.xy + p * r.zw;
    gl_Position = vec4(px.x / s.x * 2.0 - 1.0, 1.0 - px.y / s.y * 2.0, 0.0, 1.0);
}";

const FS: &str = "#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying vec2 uv;
varying vec2 lp;
uniform sampler2D t;
uniform vec2 hs;
uniform float rad;
uniform vec4 col;
uniform float mode;
uniform float a;
void main() {
    float cov = 1.0;
    if (rad > 0.0) {
        vec2 q = abs(lp) - hs + vec2(rad);
        float d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - rad;
        cov = clamp(0.5 - d, 0.0, 1.0);
    }
    vec4 c = mode > 0.5 ? col : texture2D(t, uv) * a;
    gl_FragColor = c * cov;
}";

struct Tex {
    t: glow::Texture,
    w: u32,
    h: u32,
    used: u64,
}

pub struct GlGfx {
    gl: glow::Context,
    prog: glow::Program,
    vbo: glow::Buffer,
    u_s: Option<glow::UniformLocation>,
    u_r: Option<glow::UniformLocation>,
    u_hs: Option<glow::UniformLocation>,
    u_rad: Option<glow::UniformLocation>,
    u_col: Option<glow::UniformLocation>,
    u_mode: Option<glow::UniformLocation>,
    u_a: Option<glow::UniformLocation>,
    tex: HashMap<String, Tex>,
    frame: u64,
    size: (u32, u32),
}

impl GlGfx {
    pub fn new(gl: glow::Context) -> Result<GlGfx, String> {
        unsafe {
            let sh = |ty, src: &str| -> Result<glow::Shader, String> {
                let s = gl.create_shader(ty)?;
                gl.shader_source(s, src);
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    return Err(gl.get_shader_info_log(s));
                }
                Ok(s)
            };
            let vs = sh(glow::VERTEX_SHADER, VS)?;
            let fs = sh(glow::FRAGMENT_SHADER, FS)?;
            let prog = gl.create_program()?;
            gl.attach_shader(prog, vs);
            gl.attach_shader(prog, fs);
            gl.bind_attrib_location(prog, 0, "p");
            gl.link_program(prog);
            if !gl.get_program_link_status(prog) {
                return Err(gl.get_program_info_log(prog));
            }
            let vbo = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let v: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, std::slice::from_raw_parts(v.as_ptr() as *const u8, 32), glow::STATIC_DRAW);
            let loc = |n: &str| gl.get_uniform_location(prog, n);
            Ok(GlGfx {
                u_s: loc("s"),
                u_r: loc("r"),
                u_hs: loc("hs"),
                u_rad: loc("rad"),
                u_col: loc("col"),
                u_mode: loc("mode"),
                u_a: loc("a"),
                gl,
                prog,
                vbo,
                tex: HashMap::new(),
                frame: 0,
                size: (1, 1),
            })
        }
    }

    pub fn renderer(&self) -> String {
        unsafe { self.gl.get_parameter_string(glow::RENDERER) }
    }

    fn begin(&mut self, w: u32, h: u32) {
        self.frame += 1;
        self.size = (w, h);
        unsafe {
            let gl = &self.gl;
            gl.viewport(0, 0, w as i32, h as i32);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.prog));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.active_texture(glow::TEXTURE0);
            gl.uniform_2_f32(self.u_s.as_ref(), w as f32, h as f32);
        }
    }

    fn end_frame(&mut self) {
        if self.tex.len() <= 400 {
            return;
        }
        let limit = self.frame.saturating_sub(300);
        let old: Vec<String> = self.tex.iter().filter(|(_, e)| e.used < limit).map(|(k, _)| k.clone()).collect();
        for k in old {
            if let Some(e) = self.tex.remove(&k) {
                unsafe { self.gl.delete_texture(e.t) };
            }
        }
    }

    fn upload(&mut self, key: &str, pm: &Pixmap) -> Option<()> {
        let (w, h) = (pm.width(), pm.height());
        unsafe {
            let gl = &self.gl;
            let t = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(t));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA as i32, w as i32, h as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(pm.data())));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            self.tex.insert(key.to_string(), Tex { t, w, h, used: self.frame });
        }
        Some(())
    }

    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, rad: f32, col: [f32; 4], alpha: f32, tex: Option<glow::Texture>) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let rad = rad.max(0.0).min(w.min(h) / 2.0);
        unsafe {
            let gl = &self.gl;
            gl.uniform_4_f32(self.u_r.as_ref(), x, y, w, h);
            gl.uniform_2_f32(self.u_hs.as_ref(), w / 2.0, h / 2.0);
            gl.uniform_1_f32(self.u_rad.as_ref(), rad);
            gl.uniform_4_f32(self.u_col.as_ref(), col[0], col[1], col[2], col[3]);
            gl.uniform_1_f32(self.u_a.as_ref(), alpha);
            match tex {
                Some(t) => {
                    gl.uniform_1_f32(self.u_mode.as_ref(), 0.0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(t));
                }
                None => gl.uniform_1_f32(self.u_mode.as_ref(), 1.0),
            }
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }
}

fn premul(c: Color) -> [f32; 4] {
    let a = c.alpha();
    [c.red() * a, c.green() * a, c.blue() * a, a]
}

fn solid(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = r * 0.552_284_7;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Surface de dessin : GPU (OpenGL ES) ou CPU (tiny-skia), mêmes primitives dans les deux cas.
pub enum Gfx {
    Cpu { canvas: Pixmap, imgs: HashMap<String, Pixmap> },
    Gl(Box<GlGfx>),
}

impl Gfx {
    pub fn new_cpu(w: u32, h: u32) -> Gfx {
        Gfx::Cpu { canvas: Pixmap::new(w.max(1), h.max(1)).unwrap(), imgs: HashMap::new() }
    }


    /// Taille (px) de la surface courante.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Gfx::Gl(g) => g.size,
            Gfx::Cpu { canvas, .. } => (canvas.width(), canvas.height()),
        }
    }

    /// Remplit toute la surface.
    pub fn clear(&mut self, c: Color) {
        let (w, h) = self.size();
        self.rect(0.0, 0.0, w as f32, h as f32, c);
    }

    /// Disque de centre (cx, cy) et de rayon r.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        self.rrect(cx - r, cy - r, r * 2.0, r * 2.0, r, c);
    }

    /// Image mise en cache sous `key` (envoyée une seule fois au GPU), dessinée en (x, y).
    pub fn image(&mut self, key: &str, pm: &Pixmap, x: f32, y: f32, alpha: f32) {
        self.img(key, &|| pm.clone());
        self.blit(key, x, y, alpha);
    }

    pub fn canvas(&self) -> Option<&Pixmap> {
        match self {
            Gfx::Cpu { canvas, .. } => Some(canvas),
            Gfx::Gl(_) => None,
        }
    }

    pub fn begin(&mut self, w: u32, h: u32) {
        match self {
            Gfx::Gl(g) => g.begin(w, h),
            Gfx::Cpu { canvas, .. } => {
                if canvas.width() != w || canvas.height() != h {
                    if let Some(p) = Pixmap::new(w.max(1), h.max(1)) {
                        *canvas = p;
                    }
                }
            }
        }
    }

    pub fn end_frame(&mut self) {
        if let Gfx::Gl(g) = self {
            g.end_frame();
        }
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        match self {
            Gfx::Gl(g) => g.quad(x, y, w, h, 0.0, premul(c), 1.0, None),
            Gfx::Cpu { canvas, .. } => {
                if let Some(r) = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) {
                    canvas.fill_rect(r, &solid(c), Transform::identity(), None);
                }
            }
        }
    }

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        match self {
            Gfx::Gl(g) => g.quad(x, y, w, h, r, premul(c), 1.0, None),
            Gfx::Cpu { canvas, .. } => {
                if let Some(p) = rrect_path(x, y, w, h, r) {
                    canvas.fill_path(&p, &solid(c), FillRule::Winding, Transform::identity(), None);
                }
            }
        }
    }

    /// S'assure que l'image `key` est en cache (créée par `make` sinon) ; renvoie sa taille.
    pub fn img(&mut self, key: &str, make: &dyn Fn() -> Pixmap) -> (u32, u32) {
        match self {
            Gfx::Cpu { imgs, .. } => {
                if imgs.len() > 400 {
                    imgs.clear();
                }
                let pm = imgs.entry(key.to_string()).or_insert_with(make);
                (pm.width(), pm.height())
            }
            Gfx::Gl(g) => {
                let frame = g.frame;
                if let Some(e) = g.tex.get_mut(key) {
                    e.used = frame;
                    return (e.w, e.h);
                }
                let pm = make();
                let sz = (pm.width(), pm.height());
                g.upload(key, &pm);
                sz
            }
        }
    }

    /// Dessine l'image `key` (déjà en cache) à sa taille naturelle, coin haut-gauche en (x, y).
    pub fn blit(&mut self, key: &str, x: f32, y: f32, alpha: f32) {
        let (x, y) = (x.round(), y.round());
        match self {
            Gfx::Gl(g) => {
                let frame = g.frame;
                let e = g.tex.get_mut(key).map(|e| {
                    e.used = frame;
                    (e.t, e.w, e.h)
                });
                if let Some((t, w, h)) = e {
                    g.quad(x, y, w as f32, h as f32, 0.0, [0.0; 4], alpha, Some(t));
                }
            }
            Gfx::Cpu { canvas, imgs } => {
                if let Some(pm) = imgs.get(key) {
                    let paint = PixmapPaint { opacity: alpha, ..PixmapPaint::default() };
                    canvas.draw_pixmap(x as i32, y as i32, pm.as_ref(), &paint, Transform::identity(), None);
                }
            }
        }
    }
}