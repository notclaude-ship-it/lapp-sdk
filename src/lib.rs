//! # lapp-sdk
//!
//! SDK pour écrire des applications au format `.lapp` (voir `../README.md` pour le format et
//! `README.md` ici pour le guide du SDK).
//!
//! Une app = une structure qui implémente [`App`], lancée par [`run`] :
//!
//! ```no_run
//! use lapp_sdk::prelude::*;
//!
//! struct Hello { clicks: u32 }
//!
//! impl App for Hello {
//!     fn render(&mut self, ctx: &Ctx, g: &mut Gfx) {
//!         g.clear(ctx.theme.bg);
//!         let r = Rect::new(40.0, 200.0, ctx.width() - 80.0, 56.0);
//!         ui::button(ctx, g, r, "Hello", ButtonStyle::Primary, false);
//!     }
//!     fn event(&mut self, ctx: &Ctx, ev: Event) {
//!         if let Event::Tap { .. } = ev {
//!             self.clicks += 1;
//!             ctx.toast(format!("{} clics", self.clicks));
//!         }
//!     }
//! }
//!
//! fn main() {
//!     run(Config::new("Hello"), Hello { clicks: 0 });
//! }
//! ```

pub mod audio;
mod gfx;
mod input;
mod kbd;
mod runtime;
mod store;
mod sys;
mod text;
pub mod ui;

pub use gfx::Gfx;
pub use input::{Event, Key, Scroller};
pub use runtime::{icon_png, render_png, run, App, Config, Ctx};
pub use store::Store;
pub use sys::{app_dir, app_path, arg_flag, arg_value, data_dir, in_sandbox, lapp_bin, load_png};
pub use text::{Align, Fonts, Kind, Text};
pub use tiny_skia::{self, Color, Pixmap};
pub use ui::{rgb, rgba, shade, with_alpha, ButtonStyle, Rect, Theme};

/// Tout le nécessaire en un `use lapp_sdk::prelude::*;`.
pub mod prelude {
    pub use crate::ui;
    pub use crate::{
        rgb, rgba, run, shade, with_alpha, App, Align, ButtonStyle, Color, Config, Ctx, Event, Gfx, Key, Pixmap, Rect, Scroller, Store, Text,
        Theme,
    };
}