//The default paths come from src/default_paths.txt, which is personal (and gitignored). Without one, the sample
//is used, so a fresh copy still builds.
use std::path::Path;

fn main() {
    let src = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");
    let personal = src.join("default_paths.txt");
    let paths = if personal.exists() {
        println!("cargo:rerun-if-changed={}", personal.display());
        personal
    } else {
        //Watching the folder notices the personal file being added, though any change in it reruns this
        println!("cargo:rerun-if-changed={}", src.display());
        src.join("default_paths_SAMPLE.txt")
    };
    println!("cargo:rustc-env=OMCLONE_DEFAULT_PATHS={}", paths.display());
}
