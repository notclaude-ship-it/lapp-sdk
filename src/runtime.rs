//! Boucle d'exécution : fenêtre plein écran, GPU (EGL / OpenGL ES 2) avec repli CPU (softbuffer),
//! reconnaissance des gestes, toasts, captures d'écran hors écran.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowId};

use crate::gfx::{Gfx, GlGfx};
use crate::input::{Event, Key};
use crate::text::{self, Fonts, Text};
use crate::ui::{rgba, Theme};

/// Réglages de l'app (nom de fenêtre, mode de rendu, geste de sortie…).
#[derive(Clone, Debug)]
pub struct Config {
    /// Titre de la fenêtre ; sa première lettre sert aussi à l'icône par défaut.
    pub title: String,
    /// Taille de repli (px) quand aucun écran n'est détecté, et des captures par défaut.
    pub size: (u32, u32),
    /// Quitter en glissant vers le haut depuis le bord bas de l'écran (pas de gestionnaire de fenêtres
    /// sur l'appareil : sans ce geste, l'app doit gérer sa propre sortie avec `Ctx::exit`).
    pub edge_exit: bool,
    /// Essayer le rendu GPU (repli CPU automatique ; `LAPP_CPU=1` force le CPU).
    pub gpu: bool,
}

impl Config {
    pub fn new(title: &str) -> Config {
        Config { title: title.to_string(), size: (272, 480), edge_exit: true, gpu: true }
    }
    pub fn size(mut self, w: u32, h: u32) -> Self {
        self.size = (w, h);
        self
    }
    pub fn edge_exit(mut self, on: bool) -> Self {
        self.edge_exit = on;
        self
    }
    pub fn gpu(mut self, on: bool) -> Self {
        self.gpu = on;
        self
    }
}

/// Une application lapp. Seul `render` est obligatoire.
pub trait App {
    /// Dessine une image complète. Appelée après chaque événement, et en continu tant que
    /// `ctx.animate()` est appelée.
    fn render(&mut self, ctx: &Ctx, g: &mut Gfx);

    /// Entrées utilisateur (toucher, souris, clavier).
    fn event(&mut self, _ctx: &Ctx, _ev: Event) {}

    /// Appelée avant chaque image avec le temps écoulé (secondes) : animations, inertie, minuteries.
    fn tick(&mut self, _ctx: &Ctx, _dt: f32) {}

    /// Echap : renvoyer `true` si l'app a géré le retour (fermer un dialogue…), sinon l'app quitte.
    fn on_back(&mut self, _ctx: &Ctx) -> bool {
        false
    }

    /// Dessine l'icône de l'app (carré de `size` px) pour `--icon`. Par défaut : tuile + initiale.
    fn icon(&mut self, ctx: &Ctx, g: &mut Gfx, size: u32) {
        default_icon(ctx, g, size);
    }
}

/// Contexte passé à l'app : taille de l'écran, polices, thème, sortie, animation, toasts.
pub struct Ctx {
    pub fonts: Fonts,
    pub theme: Theme,
    title: String,
    size: Cell<(u32, u32)>,
    exit: Cell<bool>,
    again: Cell<bool>,
    wake: Cell<Option<Instant>>,
    toast: RefCell<Option<(String, Instant)>>,
}

impl Ctx {
    fn new(cfg: &Config) -> Ctx {
        Ctx {
            fonts: Fonts::load(),
            theme: Theme::light(),
            title: cfg.title.clone(),
            size: Cell::new(cfg.size),
            exit: Cell::new(false),
            again: Cell::new(false),
            wake: Cell::new(None),
            toast: RefCell::new(None),
        }
    }

    /// Taille de la surface de dessin en pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size.get()
    }
    pub fn width(&self) -> f32 {
        self.size.get().0 as f32
    }
    pub fn height(&self) -> f32 {
        self.size.get().1 as f32
    }
    /// Facteur d'échelle d'interface : 1.0 pour un écran de 272 px de large (celui de l'appareil).
    pub fn scale(&self) -> f32 {
        self.size.get().0 as f32 / 272.0
    }
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Quitte l'app après l'image en cours.
    pub fn exit(&self) {
        self.exit.set(true);
    }

    /// Demande une image de plus (à appeler à chaque image tant qu'une animation tourne).
    pub fn animate(&self) {
        self.again.set(true);
    }

    /// Demande une image dans `d` (minuterie sans boucle continue) : à appeler dans `tick` ou `render`
    /// pour une animation lente ou un rafraîchissement périodique (le plus proche délai l'emporte).
    pub fn redraw_in(&self, d: Duration) {
        let at = Instant::now() + d;
        match self.wake.get() {
            Some(w) if w <= at => {}
            _ => self.wake.set(Some(at)),
        }
    }

    /// Affiche un message bref en bas de l'écran (2,5 s).
    pub fn toast(&self, msg: impl Into<String>) {
        *self.toast.borrow_mut() = Some((msg.into(), Instant::now() + Duration::from_millis(2500)));
    }

    /// Largeur (px) qu'occuperait `s` dans le style `st`.
    pub fn measure(&self, s: &str, st: &Text) -> f32 {
        text::measure(&self.fonts, s, st)
    }

    /// Hauteur d'une ligne de texte pour le style `st`.
    pub fn line_height(&self, st: &Text) -> f32 {
        text::line_height(&self.fonts, st)
    }

    /// Dessine `s` ; `x` = bord gauche / centre / bord droit selon `st.align`, `y` = haut de la ligne.
    pub fn text(&self, g: &mut Gfx, s: &str, x: f32, y: f32, st: &Text) {
        if s.is_empty() {
            return;
        }
        let key = st.key(s);
        let (w, _) = g.img(&key, &|| text::render(&self.fonts, s, st));
        let tw = w as f32 - 3.0;
        let left = match st.align {
            text::Align::Left => x,
            text::Align::Center => x - tw / 2.0,
            text::Align::Right => x - tw,
        };
        g.blit(&key, left - 1.0, y - 1.0, 1.0);
    }

    /// Comme `text`, mais `cy` est le milieu vertical de la ligne.
    pub fn text_mid(&self, g: &mut Gfx, s: &str, x: f32, cy: f32, st: &Text) {
        let lh = self.line_height(st);
        self.text(g, s, x, cy - lh / 2.0, st);
    }
}

fn draw_toast(ctx: &Ctx, g: &mut Gfx) {
    let t = ctx.toast.borrow();
    let Some((msg, until)) = t.as_ref() else { return };
    if Instant::now() >= *until {
        return;
    }
    let (w, h) = ctx.size();
    let (w, h) = (w as f32, h as f32);
    let s = ctx.scale();
    let st = Text::new(12.0 * s).color(rgba(0xFFFFFF, 1.0)).center().max_width(w * 0.8);
    let bw = ctx.measure(msg, &st).min(w * 0.8) + 24.0 * s;
    let bh = 26.0 * s;
    let (x, y) = ((w - bw) / 2.0, h * 0.9 - bh);
    g.rrect(x, y, bw, bh, bh / 2.0, rgba(0x000000, 0.78));
    ctx.text_mid(g, msg, w / 2.0, y + bh / 2.0, &st);
}

fn default_icon(ctx: &Ctx, g: &mut Gfx, size: u32) {
    let f = size as f32;
    let t = ctx.theme;
    g.rrect(0.0, 0.0, f, f, f * 0.22, t.accent);
    let letter: String = ctx.title().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    let st = Text::new(f * 0.56).bold().color(rgba(0xFFFFFF, 1.0)).center();
    ctx.text_mid(g, &letter, f / 2.0, f * 0.52, &st);
}

/// Rend une image de l'app dans un PNG (CPU, sans fenêtre) : tests et captures.
pub fn render_png<A: App>(app: &mut A, cfg: &Config, size: (u32, u32), path: &str) -> Result<(), String> {
    let ctx = Ctx::new(cfg);
    ctx.size.set(size);
    let mut g = Gfx::new_cpu(size.0, size.1);
    g.begin(size.0, size.1);
    app.tick(&ctx, 0.0);
    app.render(&ctx, &mut g);
    draw_toast(&ctx, &mut g);
    g.canvas().ok_or("pas de surface CPU")?.save_png(path).map_err(|e| e.to_string())
}

/// Écrit l'icône de l'app (128x128) dans un PNG.
pub fn icon_png<A: App>(app: &mut A, cfg: &Config, path: &str) -> Result<(), String> {
    let ctx = Ctx::new(cfg);
    let n = 128;
    ctx.size.set((n, n));
    let mut g = Gfx::new_cpu(n, n);
    g.begin(n, n);
    app.icon(&ctx, &mut g, n);
    g.canvas().ok_or("pas de surface CPU")?.save_png(path).map_err(|e| e.to_string())
}

/// Lance l'app. Gère aussi les options communes de la ligne de commande :
/// `--icon out.png` (icône), `--screenshot out.png [--size 272x480]` (rendu hors écran).
pub fn run<A: App>(cfg: Config, mut app: A) {
    if let Some(out) = crate::sys::arg_value("--icon") {
        if let Err(e) = icon_png(&mut app, &cfg, &out) {
            eprintln!("{} : icône : {e}", cfg.title);
            std::process::exit(1);
        }
        return;
    }
    if let Some(out) = crate::sys::arg_value("--screenshot") {
        let size = crate::sys::arg_value("--size")
            .and_then(|s| {
                let (a, b) = s.split_once('x')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            })
            .unwrap_or(cfg.size);
        if let Err(e) = render_png(&mut app, &cfg, size, &out) {
            eprintln!("{} : capture : {e}", cfg.title);
            std::process::exit(1);
        }
        return;
    }

    let el = match EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{} : pas d'affichage disponible : {e}", cfg.title);
            std::process::exit(1);
        }
    };
    el.set_control_flow(ControlFlow::Wait);
    let mut runner = Runner {
        ctx: Ctx::new(&cfg),
        cfg,
        app,
        window: None,
        surface: None,
        gl: None,
        cpu: None,
        surf_size: (0, 0),
        cursor: (0.0, 0.0),
        press: None,
        last_frame: Instant::now(),
    };
    if let Err(e) = el.run_app(&mut runner) {
        eprintln!("{} : {e}", runner.cfg.title);
        std::process::exit(1);
    }
}

struct Press {
    start: (f32, f32),
    last: (f32, f32),
    last_t: Instant,
    vx: f32,
    vy: f32,
    dragging: bool,
    /// Geste commencé sur le bord bas (réservé à la sortie quand `edge_exit` est actif).
    edge: bool,
}

struct GlState {
    surface: Surface<WindowSurface>,
    ctx: PossiblyCurrentContext,
    gfx: Gfx,
}

struct Runner<A: App> {
    cfg: Config,
    ctx: Ctx,
    app: A,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    gl: Option<GlState>,
    cpu: Option<Gfx>,
    surf_size: (u32, u32),
    cursor: (f32, f32),
    press: Option<Press>,
    last_frame: Instant,
}

impl<A: App> Runner<A> {
    fn redraw_later(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn dispatch(&mut self, ev: Event) {
        self.app.event(&self.ctx, ev);
        self.redraw_later();
    }

    fn down(&mut self, x: f32, y: f32) {
        let h = self.ctx.height();
        let edge = self.cfg.edge_exit && y > h * 0.94;
        self.press = Some(Press { start: (x, y), last: (x, y), last_t: Instant::now(), vx: 0.0, vy: 0.0, dragging: false, edge });
        self.dispatch(Event::Down { x, y });
    }

    fn moved(&mut self, x: f32, y: f32) {
        let s = self.ctx.scale();
        let Some(p) = self.press.as_mut() else { return };
        if !p.dragging && ((x - p.start.0).abs() > 9.0 * s || (y - p.start.1).abs() > 9.0 * s) {
            p.dragging = true;
        }
        let now = Instant::now();
        let dt = (now - p.last_t).as_secs_f32().max(0.001);
        let (dx, dy) = (x - p.last.0, y - p.last.1);
        p.vx = 0.65 * (dx / dt) + 0.35 * p.vx;
        p.vy = 0.65 * (dy / dt) + 0.35 * p.vy;
        p.last = (x, y);
        p.last_t = now;
        let (dragging, edge) = (p.dragging, p.edge);
        self.dispatch(Event::Move { x, y });
        if dragging && !edge {
            self.dispatch(Event::Drag { x, y, dx, dy });
        }
    }

    fn up(&mut self, x: f32, y: f32) {
        let Some(p) = self.press.take() else { return };
        let h = self.ctx.height();
        self.dispatch(Event::Up { x, y });
        if p.edge && p.start.1 - y > h * 0.08 {
            self.ctx.exit(); // glisser vers le haut depuis le bas de l'écran
            return;
        }
        if !p.dragging {
            self.dispatch(Event::Tap { x, y });
        } else if !p.edge {
            let fresh = p.last_t.elapsed() < Duration::from_millis(90);
            let (vx, vy) = if fresh { (p.vx, p.vy) } else { (0.0, 0.0) };
            self.dispatch(Event::DragEnd { x, y, vx, vy });
        }
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;

        let Some(window) = self.window.clone() else { return };
        let sz = window.inner_size();
        let (Some(nw), Some(nh)) = (NonZeroU32::new(sz.width), NonZeroU32::new(sz.height)) else { return };
        self.ctx.size.set((sz.width, sz.height));
        self.ctx.again.set(false);
        self.ctx.wake.set(None);
        self.app.tick(&self.ctx, dt);

        if let Some(mut gs) = self.gl.take() {
            if self.surf_size != (sz.width, sz.height) {
                gs.surface.resize(&gs.ctx, nw, nh);
                self.surf_size = (sz.width, sz.height);
            }
            gs.gfx.begin(sz.width, sz.height);
            self.app.render(&self.ctx, &mut gs.gfx);
            draw_toast(&self.ctx, &mut gs.gfx);
            gs.gfx.end_frame();
            let _ = gs.surface.swap_buffers(&gs.ctx);
            self.gl = Some(gs);
        } else {
            let mut g = self.cpu.take().unwrap_or_else(|| Gfx::new_cpu(sz.width, sz.height));
            g.begin(sz.width, sz.height);
            self.app.render(&self.ctx, &mut g);
            draw_toast(&self.ctx, &mut g);
            if let (Some(pm), Some(surface)) = (g.canvas(), self.surface.as_mut()) {
                if surface.resize(nw, nh).is_ok() {
                    if let Ok(mut buf) = surface.buffer_mut() {
                        for (dst, px) in buf.iter_mut().zip(pm.data().chunks_exact(4)) {
                            *dst = ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32;
                        }
                        let _ = buf.present();
                    }
                }
            }
            self.cpu = Some(g);
        }
        if self.ctx.again.get() {
            window.request_redraw();
        }
    }

    /// Crée la fenêtre et un contexte OpenGL ES (EGL) ; None si le GPU n'est pas utilisable (repli CPU).
    fn try_gl(&mut self, el: &ActiveEventLoop, attrs: winit::window::WindowAttributes) -> Option<Rc<Window>> {
        if !self.cfg.gpu || std::env::var("LAPP_CPU").is_ok() {
            return None;
        }
        let name = self.cfg.title.clone();
        let built = DisplayBuilder::new().with_window_attributes(Some(attrs)).build(
            el,
            ConfigTemplateBuilder::new(),
            |c| c.reduce(|a, b| if b.num_samples() < a.num_samples() { b } else { a }).unwrap(),
        );
        let (window, config) = match built {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{name} : EGL indisponible, repli CPU : {e}");
                return None;
            }
        };
        let window = window?;
        let raw = window.window_handle().ok()?.as_raw();
        let display = config.display();
        let ca = ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(Some(Version::new(2, 0)))).build(Some(raw));
        let nc = match unsafe { display.create_context(&config, &ca) } {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{name} : contexte GLES impossible, repli CPU : {e}");
                return None;
            }
        };
        let sa = window.build_surface_attributes(Default::default()).ok()?;
        let surface = match unsafe { display.create_window_surface(&config, &sa) } {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{name} : surface EGL impossible, repli CPU : {e}");
                return None;
            }
        };
        let ctx = nc.make_current(&surface).ok()?;
        let _ = surface.set_swap_interval(&ctx, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let gl = unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };
        let g = match GlGfx::new(gl) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("{name} : shaders GLES refusés, repli CPU : {e}");
                return None;
            }
        };
        let rn = g.renderer();
        let low = rn.to_lowercase();
        if low.contains("llvmpipe") || low.contains("softpipe") || low.contains("swrast") {
            // rendu logiciel de Mesa : plus lent que le CPU maison
            eprintln!("{name} : EGL sans GPU réel ({rn}), repli CPU");
            return None;
        }
        eprintln!("{name} : rendu GPU ({rn})");
        self.gl = Some(GlState { surface, ctx, gfx: Gfx::Gl(Box::new(g)) });
        Some(Rc::new(window))
    }
}

impl<A: App> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // pas de gestionnaire de fenêtres : on prend toute la taille de l'écran
        let (w, h) = el
            .primary_monitor()
            .or_else(|| el.available_monitors().next())
            .map(|m| (m.size().width, m.size().height))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or(self.cfg.size);
        let attrs = Window::default_attributes()
            .with_title(self.cfg.title.clone())
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(w, h))
            .with_position(PhysicalPosition::new(0, 0));
        let name = self.cfg.title.clone();
        let window = match self.try_gl(el, attrs.clone()) {
            Some(w) => w,
            None => {
                let window = match el.create_window(attrs) {
                    Ok(win) => Rc::new(win),
                    Err(e) => {
                        eprintln!("{name} : fenêtre impossible : {e}");
                        el.exit();
                        return;
                    }
                };
                let ctx = match softbuffer::Context::new(window.clone()) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("{name} : contexte graphique : {e}");
                        el.exit();
                        return;
                    }
                };
                match softbuffer::Surface::new(&ctx, window.clone()) {
                    Ok(s) => self.surface = Some(s),
                    Err(e) => {
                        eprintln!("{name} : surface graphique : {e}");
                        el.exit();
                        return;
                    }
                }
                eprintln!("{name} : rendu CPU");
                window
            }
        };
        self.ctx.size.set((w, h));
        self.last_frame = Instant::now();
        window.request_redraw();
        self.window = Some(window);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.ctx.exit.get() {
            el.exit();
            return;
        }
        let mut until = self.ctx.toast.borrow().as_ref().map(|(_, u)| *u);
        if let Some(w) = self.ctx.wake.get() {
            if Instant::now() >= w {
                self.ctx.wake.set(None);
                self.redraw_later();
            } else {
                until = Some(until.map_or(w, |u| u.min(w)));
            }
        }
        match until {
            Some(u) if Instant::now() >= u => {
                *self.ctx.toast.borrow_mut() = None;
                self.redraw_later();
                el.set_control_flow(ControlFlow::Wait);
            }
            Some(u) => el.set_control_flow(ControlFlow::WaitUntil(u)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(_) => self.redraw_later(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. } => {
                let key = match logical_key {
                    WKey::Named(NamedKey::Escape) => {
                        if !self.app.on_back(&self.ctx) {
                            self.ctx.exit();
                        }
                        self.redraw_later();
                        None
                    }
                    WKey::Named(NamedKey::Backspace) => Some(Key::Back),
                    WKey::Named(NamedKey::Enter) => Some(Key::Enter),
                    WKey::Named(NamedKey::Space) => Some(Key::Space),
                    WKey::Named(NamedKey::ArrowUp) => Some(Key::Up),
                    WKey::Named(NamedKey::ArrowDown) => Some(Key::Down),
                    WKey::Named(NamedKey::ArrowLeft) => Some(Key::Left),
                    WKey::Named(NamedKey::ArrowRight) => Some(Key::Right),
                    WKey::Character(s) => s.chars().next().map(Key::Char),
                    _ => None,
                };
                if let Some(k) = key {
                    self.dispatch(Event::Key(k));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 48.0 * self.ctx.scale(),
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                self.dispatch(Event::Wheel { dy });
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                self.moved(self.cursor.0, self.cursor.1);
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => self.down(self.cursor.0, self.cursor.1),
                ElementState::Released => self.up(self.cursor.0, self.cursor.1),
            },
            WindowEvent::Touch(t) => {
                let (x, y) = (t.location.x as f32, t.location.y as f32);
                self.cursor = (x, y);
                match t.phase {
                    TouchPhase::Started => self.down(x, y),
                    TouchPhase::Moved => self.moved(x, y),
                    TouchPhase::Ended | TouchPhase::Cancelled => self.up(x, y),
                }
            }
            _ => {}
        }
        if self.ctx.exit.get() {
            el.exit();
        }
    }
}