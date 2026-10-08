use omclone::{parser, sim, timeline};

#[cfg(any(feature = "editor_ui",feature = "display_ui",))]
mod render_library;
#[cfg(any(feature = "editor_ui",feature = "display_ui",))]
mod render_glyphs;
#[cfg(any(feature = "editor_ui",feature = "display_ui",))]
mod render_sim;
#[cfg(any(feature = "editor_ui",feature = "display_ui",))]
mod ui;

#[cfg(feature = "color-eyre")]
use color_eyre::{install, eyre::Result};
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::{install, eyre::Result};

#[cfg(any(feature = "editor_ui",feature = "display_ui",))]
fn main() -> Result< () >{
    #[cfg(not(target_arch = "wasm32"))]
    std::env::set_var("RUST_BACKTRACE", "full");
    install()?;

    use miniquad::*;
    let conf = conf::Conf{
        window_title: String::from("omclone"),
        window_width: 1600,
        window_height: 900,
        //Sharp text on scaled displays. The board works in logical pixels, so it looks the same either way.
        high_dpi: true,
        fullscreen: false,
        .. Default::default()
    };
    miniquad::start(conf, || {
        Box::new(ui::MyMiniquadApp::new())
    });
    Ok(())
}
