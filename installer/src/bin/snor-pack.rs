//! The packer: `snor-pack <snor-setup.exe> <snor.exe> <out.exe>`.
//!
//! ```text
//! cargo build --release                      # the app
//! cargo build --release -p snor-installer    # the wizard and this packer
//! target/release/snor-pack.exe target/release/snor-setup.exe target/release/snor.exe dist/SnorSetup.exe
//! ```
//!
//! produces the one file that gets handed out. `--verify <file>` reports what
//! a setup binary would find inside itself, which is how a bundle is checked
//! without running it.

use std::path::Path;

use snor_installer::payload;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [flag, file] if flag == "--verify" => verify(Path::new(file)),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", usage());
            Ok(())
        }
        [setup, app, out] => pack(Path::new(setup), Path::new(app), Path::new(out)),
        _ => Err(usage()),
    };
    if let Err(message) = result {
        eprintln!("snor-pack: {message}");
        std::process::exit(1);
    }
}

fn usage() -> String {
    "usage: snor-pack <snor-setup.exe> <snor.exe> <out.exe>\n       snor-pack --verify <file>"
        .to_string()
}

fn pack(setup: &Path, app: &Path, out: &Path) -> Result<(), String> {
    let app_bytes =
        std::fs::read(app).map_err(|e| format!("could not read {}: {e}", app.display()))?;
    let total = payload::bundle(setup, &app_bytes, out)?;
    println!(
        "wrote {}: {total} bytes ({} payload)",
        out.display(),
        app_bytes.len()
    );
    Ok(())
}

fn verify(file: &Path) -> Result<(), String> {
    match payload::parse_file(file)? {
        Some(bundle) => {
            println!(
                "{}: payload {} bytes, checksum ok, setup prefix {} bytes",
                file.display(),
                bundle.exe.len(),
                bundle.prefix_len
            );
            Ok(())
        }
        None => Err(format!(
            "{}: no payload (this file is not a bundle)",
            file.display()
        )),
    }
}
