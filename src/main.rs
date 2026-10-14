use omclone::{parser, rules, sim, timeline};

mod render_library;
mod render_glyphs;
mod render_sim;
mod ui;

#[cfg(feature = "color-eyre")]
use color_eyre::{install, eyre::Result};
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::{install, eyre::Result};

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
