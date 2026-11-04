//! Diagnostic: the automation write (cap-std, relative to a held directory) against a folder, e.g. a network share.
//! `cargo run -p photocraft-automation --example nas_probe -- "<write root>" "<relative file>"`
use photocraft_automation::AuthorizedWorkspace;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("write root is required")?);
    let name = args.next().ok_or("relative file is required")?;
    let workspace = AuthorizedWorkspace::new(None, Some(&root))?;
    println!("write new:      {:?}", workspace.write(&name, &vec![b'x'; 31_528]));
    println!("write replace:  {:?}", workspace.write(&name, &vec![b'y'; 31_528]));
    Ok(())
}
