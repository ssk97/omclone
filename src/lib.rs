//The simulation and the editor's timeline over it, the file formats and an omsim-style verifier, without any UI.
//The app, the verify library and the tests share them.
pub mod parser;
pub mod rules;
pub mod sim;
pub mod timeline;
pub mod verifier;
#[cfg(test)]
mod test;
