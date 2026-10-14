//The simulation and the file formats, without any UI. The app and the tests share them.
pub mod parser;
pub mod sim;
pub mod timeline;
#[cfg(test)]
mod test;

//The base folder, puzzle and solution to load at first, from src/default_paths.txt
//(or the sample, if there isn't one; see build.rs)
pub fn default_paths() -> (&'static str, &'static str, &'static str){
    let mut lines = include_str!(env!("OMCLONE_DEFAULT_PATHS")).lines();
    let mut next = || lines.next().unwrap_or("");
    (next(), next(), next())
}
