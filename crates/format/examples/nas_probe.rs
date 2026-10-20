//! Diagnostic: run the real atomic write against a folder (a network share) and say which step fails.
//! `cargo run -p photocraft-format --example nas_probe -- "<folder>" "<file name>"`
use photocraft_format::atomic::{self, AtomicFs, RealFs};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("folder"));
    let name = args.next().expect("file name");
    let target = dir.join(&name);
    let bytes = vec![b'x'; 31_528];
    println!("target exists before: {} (Path::exists)", target.exists());
    println!("atomic_write: {:?}", atomic::atomic_write(&target, &bytes));
    println!("target exists after: {}", target.exists());
    // Again over the file just made: the replace-an-existing-file path.
    println!("atomic_write again (replacing): {:?}", atomic::atomic_write(&target, &bytes));
    // The steps on their own.
    let tmp = dir.join(atomic::temp_name(&name));
    println!("write_new: {:?}", RealFs.write_new(&tmp, &bytes));
    let dest2 = dir.join(format!("{name}.second"));
    println!("rename (new destination): {:?}", RealFs.rename(&tmp, &dest2));
    let tmp2 = dir.join(atomic::temp_name(&name));
    println!("write_new 2: {:?}", RealFs.write_new(&tmp2, &bytes));
    println!("replace_existing (over {}): {:?}", dest2.display(), RealFs.replace_existing(&tmp2, &dest2));
}
