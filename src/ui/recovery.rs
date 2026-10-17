//An emergency save: if the editor panics, the unsaved solution is written under a new name next to the original
//(downloaded, on the web), so neither it nor an earlier emergency save is overwritten
use super::*;
use std::sync::Mutex;

//The unsaved solution, serialized, and where Save would write it
static BACKUP: Mutex<Option<(Vec<u8>, PathBuf)>> = Mutex::new(None);

pub fn store(data: Vec<u8>, path: PathBuf){
    *BACKUP.lock().unwrap() = Some((data, path));
}
pub fn clear(){
    *BACKUP.lock().unwrap() = None;
}
fn recovered_name(path: &std::path::Path, copy: usize) -> String{
    let stem = path.file_stem().map_or("omclone".into(), |s| s.to_string_lossy());
    match copy{
        1 => format!("{stem}.recovered.solution"),
        n => format!("{stem}.recovered-{n}.solution"),
    }
}
pub fn install_panic_hook(){
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        //Skipped if the panic came while storing a backup, which holds the lock. Taken so it's saved once: on the
        //web, every event that reaches the app after a panic panics again.
        let saved = BACKUP.try_lock().ok().and_then(|mut backup| backup.take()).map(|(data, path)| write(&data, &path));
        previous(info);
        match saved{
            Some(Ok(at)) => eprintln!("Unsaved changes were saved to {at}"),
            Some(Err(err)) => eprintln!("Unsaved changes couldn't be saved: {err}"),
            None => {}
        }
    }));
}
#[cfg(not(target_arch = "wasm32"))]
fn write(data: &[u8], path: &std::path::Path) -> std::io::Result<String>{
    for copy in 1..{
        let at = path.with_file_name(recovered_name(path, copy));
        //create_new never replaces an existing file
        match std::fs::File::create_new(&at){
            Ok(mut file) => {
                file.write_all(data)?;
                return Ok(at.display().to_string());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    unreachable!()
}
//The browser renames a download rather than replace a file
#[cfg(target_arch = "wasm32")]
fn write(data: &[u8], path: &std::path::Path) -> std::io::Result<String>{
    let name = recovered_name(path, 1);
    download(data, &name);
    Ok(name)
}
