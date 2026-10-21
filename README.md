# lapp-sdk

SDK Rust pour écrire des applications au format `.lapp` (voir `../README.md` pour le format, la signature et le bac à sable). Il remplace le code répété dans les exemples (`hello-world`, `file-browser`, `settings`) : fenêtre plein écran, rendu GPU (OpenGL ES) avec repli CPU, toucher/souris, texte, stockage, composants d'interface, outils de build.

Cible : `aarch64` (appareil), développement possible sur PC x86_64.

## Démarrage rapide

```sh
~/lapp/sdk/tools/lapp-new com.moi.notes --name Notes   # crée ./notes (+ icon.png)
cd notes
cargo run -- --screenshot out.png                      # aperçu hors écran (CPU, sans fenêtre)
~/lapp/sdk/tools/lapp-build                            # compile en aarch64 et produit com.moi.notes-1.0.0.lapp
sudo ~/lapp/target/release/lapp install com.moi.notes-1.0.0.lapp
```

Exemple complet : `examples/sdkdemo` (généré par `lapp-new com.exemple.sdkdemo`).

Prérequis (déjà en place sur natalos) : `rustup target add aarch64-unknown-linux-gnu`, `gcc-aarch64-linux-gnu`, le binaire `lapp` compilé (`cargo build --release` dans `~/lapp`) et une clé `~/.lapp/dev.key` (`lapp keygen dev`).

## Outils (`sdk/tools/`)

| Outil | Rôle |
|---|---|
| `lapp-new <id> [--name N] [--dir D] [--no-icon]` | Crée un projet : `Cargo.toml`, `manifest.toml`, `src/main.rs` (compteur), `.cargo/config.toml` (linker aarch64), `icon.png` |
| `lapp-build [dossier]` | `cargo build --release --target aarch64-unknown-linux-gnu`, assemble `stage/` (binaire, manifeste, icône, `assets/`), `lapp pack`, `lapp info`. Variables : `LAPP`, `LAPP_KEY`, `LAPP_COMP` |

Le dernier élément de l'id donne le nom du paquet Cargo et du binaire (`com.moi.notes` -> `notes`, `exec = "bin/notes"`). Les fichiers de `assets/` sont copiés dans l'image et lisibles via `app_path("assets/...")`.

## Structure d'une app

```rust
use lapp_sdk::prelude::*;

struct MyApp { count: u32 }

impl App for MyApp {
    fn render(&mut self, ctx: &Ctx, g: &mut Gfx) {            // obligatoire : dessine une image complète
        g.clear(ctx.theme.bg);
        ui::header(ctx, g, "Titre", None);
    }
    fn event(&mut self, ctx: &Ctx, ev: Event) { /* toucher, clavier */ }
    fn tick(&mut self, ctx: &Ctx, dt: f32) { /* animations, minuteries */ }
    fn on_back(&mut self, ctx: &Ctx) -> bool { false }           // Echap : true = géré, false = quitter
}

fn main() {
    run(Config::new("Mon app"), MyApp { count: 0 });
}
```

`run` gère aussi `--icon out.png` et `--screenshot out.png [--size 272x480]`. Pour piloter l'état initial d'une capture, lisez vos propres options avec `arg_value("--page")` / `arg_flag("--hidden")` avant d'appeler `run`.

L'app est redessinée après chaque événement. Pour une animation continue, appelez `ctx.animate()` à chaque image (dans `tick` ou `render`) tant qu'elle dure. Pour un rafraîchissement périodique sans boucle continue (horloge, interrogation d'un service), appelez `ctx.redraw_in(Duration::from_millis(250))`.

### Quitter

Il n'y a pas de gestionnaire de fenêtres sur l'appareil : par défaut (`Config::edge_exit(true)`) glisser vers le haut depuis le bord bas de l'écran quitte l'app, et Echap appelle `on_back` (puis quitte). `ctx.exit()` quitte depuis le code. Avec `edge_exit(false)`, l'app doit prévoir sa propre sortie.

## Événements

| Événement | Quand |
|---|---|
| `Down {x,y}` / `Move {x,y}` / `Up {x,y}` | doigt ou bouton gauche (bruts) |
| `Tap {x,y}` | appui bref sans glissement : à utiliser pour les boutons |
| `Drag {x,y,dx,dy}` | glissement en cours (seuil : 9 px × `ctx.scale()`) |
| `DragEnd {x,y,vx,vy}` | fin de glissement, vitesse du doigt en px/s |
| `Wheel {dy}` | molette, même signe que `Drag::dy` |
| `Key(Key)` | `Back`, `Enter`, `Space`, flèches, `Char(c)` |

Les gestes démarrés sur le bord bas (6 % de la hauteur) ne produisent ni `Tap`, ni `Drag`, ni `DragEnd` quand `edge_exit` est actif.

**Défilement** : `Scroller` gère position, bornes et inertie.

```rust
Event::Down { .. } => self.scroll.stop(),
Event::Drag { dy, .. } | Event::Wheel { dy } => self.scroll.drag(dy),
Event::DragEnd { vy, .. } => self.scroll.fling(vy),
// tick :   if self.scroll.update(dt) { ctx.animate(); }
// render : self.scroll.set_content(hauteur_contenu, hauteur_visible); y = top - self.scroll.pos
```

## Dessin (`Gfx`)

`rect`, `rrect` (rectangle arrondi), `circle`, `clear`, `image(clé, &Pixmap, x, y, alpha)`, `size()`. Les formes sont des quads GPU avec distance signée (anti-crénelage). Les images sont envoyées une fois au GPU puis réutilisées : donnez une clé stable. Le même code dessine en CPU si le GPU est indisponible.

Couleurs : `rgb(0x007aff)`, `rgba(0x000000, 0.3)`, `with_alpha`, `shade(c, 0.85)`, et `ctx.theme` (`bg`, `surface`, `text`, `subtext`, `accent`, `danger`, `line`, `press`). `Color` et `Pixmap` viennent de `tiny-skia` (réexporté) ; `load_png("assets/x.png")` charge une image de l'app.

### Texte

```rust
let st = Text::new(16.0 * ctx.scale()).bold().color(ctx.theme.text).center().max_width(200.0);
ctx.text(g, "Bonjour", x, y_haut_de_ligne, &st);
ctx.text_mid(g, "Bonjour", x, y_milieu, &st);   // centré verticalement
let w = ctx.measure("Bonjour", &st);
```

Styles : `.bold()`, `.mono()`, `.color()`, `.left()/.center()/.right()`, `.max_width(w)` (ellipse `…`), `.ellipsis_left()`. Le texte est rastérisé une fois puis mis en cache. Polices : Liberation / DejaVu système (`LAPP_FONT` pour en forcer une).

### Taille et échelle

L'écran de l'appareil fait 272×480. `ctx.scale()` est le « dpi » commun à lapp-home et aux apps : lapp-home exporte son échelle dans `LAPP_SCALE` à chaque app qu'il lance (`/etc/lapp/scale` ou `lapp-home --scale X`), et `--scale X` / `LAPP_SCALE=X` la forcent à la main ; sans cela elle vaut `min(largeur, hauteur) / 272`. Multipliez vos dimensions "en points" par `ctx.scale()` pour rester proportionnel sur d'autres tailles (`--size 720x1280`).

## Composants (`lapp_sdk::ui`)

Dessin seul, sans état : l'app garde l'état (bouton pressé, ligne sélectionnée) et teste les zones avec `Rect::contains`.

- `button(ctx, g, rect, "Texte", ButtonStyle::Primary | Secondary | Danger, pressed)`
- `header(ctx, g, "Titre", Some("Retour"))` -> `Header { height, back: Option<Rect> }`
- `list_row(ctx, g, rect, "Titre", Some("détail"), pressed)`
- `toggle(ctx, g, rect, on)`
- `Rect` : `contains`, `inset`, `split_top`, `center`, `right`, `bottom`
- `ctx.toast("message")` : bulle de 2,5 s en bas de l'écran

## Audio (`lapp_sdk::audio`)

Nécessite la permission `audio` dans le manifeste (sockets PipeWire / PulseAudio).

- `AudioOut::new("Nom", 48_000, 2)` : flux PCM S16 entrelacé vers le serveur audio de l'hôte, via `libpulse-simple` chargée à l'exécution (aucune dépendance de compilation ; sans bibliothèque ou sans serveur, `write` renvoie `false` et l'app continue sans son).
  - `write(&[i16])` / `write_bytes(&[u8])` : bloque tant que le tampon est plein (appeler depuis un thread dédié ou un callback de décodeur)
  - `flush()` : jette l'audio en attente (seek, arrêt) ; `drain()` : attend la fin ; `.latency_ms(n)` : latence cible (150 par défaut)
  - `Send` : peut être déplacé dans un autre thread
- `audio::play_file(path) -> Option<Child>` : lance `pw-play`/`paplay`/`aplay`/`mpv`/`ffplay` sur un fichier son (`child.kill()` pour arrêter).

Exemple avec libvlc (callbacks audio) : voir `vlc.rs` de l'app VLC (`libvlc_audio_set_callbacks` + `libvlc_audio_set_format("S16N", 48000, 2)`, un `AudioOut` en `opaque`).

## Données et ressources

- `Store::open("state.conf")` : clé/valeur persistant dans `/data` (`get`, `get_i64`, `get_f32`, `get_bool`, `set`, `remove`), écriture atomique.
- `data_dir()` : `/data` dans le bac à sable ; hors bac à sable, `$LAPP_DATA` sinon `./.lapp-data`.
- `app_dir()` / `app_path("assets/x.png")` : image de l'app (`/app`) ; hors bac à sable, `$LAPP_APP` sinon le dossier courant.
- `in_sandbox()`, `lapp_bin()` (`/lapp` avec la permission `install`), `arg_value`, `arg_flag`.

## Permissions du manifeste

L'app n'a que ce que son manifeste demande (liste complète dans `../README.md`). Pour une app graphique : `display` (obligatoire pour la fenêtre), `storage`, et `gpu` pour le rendu OpenGL ES. Sans `gpu`, ou si EGL est indisponible, le SDK retombe sur le rendu CPU ; `LAPP_CPU=1` force le CPU. `gpu` partage l'espace de noms réseau (de fait l'accès réseau).

## Développer sur PC

- `cargo run -- --screenshot out.png` : rendu hors écran, idéal pour itérer sans appareil.
- `LAPP_DATA=/tmp/d cargo run` : ouvre la fenêtre sur le bureau (x11/wayland).
- Le rendu GPU est ignoré si EGL ne fournit qu'un rendu logiciel Mesa (llvmpipe) : le CPU est alors plus rapide.

## Organisation du code

- `src/lib.rs` : exports et `prelude`
- `src/runtime.rs` : `App`, `Config`, `Ctx`, boucle d'événements, fenêtre, EGL/CPU, gestes, toasts, captures
- `src/gfx.rs` : primitives GPU (shader unique) et CPU (tiny-skia)
- `src/text.rs` : polices, styles de texte, rastérisation
- `src/input.rs` : `Event`, `Key`, `Scroller`
- `src/ui.rs` : couleurs, thème, `Rect`, composants
- `src/audio.rs` : sortie audio (`AudioOut`, `play_file`)
- `src/store.rs`, `src/sys.rs` : stockage et environnement
- `template/` et `tools/` : modèle de projet, `lapp-new`, `lapp-build`