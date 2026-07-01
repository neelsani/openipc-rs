use std::fs::File;
use std::io::Write;

use crypto_box::SecretKey;
use rand_core::OsRng;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        print_help();
        return Ok(());
    }
    if !args.is_empty() {
        return Err(
            "password-derived keygen is not implemented yet; run without arguments for random keys"
                .into(),
        );
    }

    let drone_secret = SecretKey::generate(&mut OsRng);
    let gs_secret = SecretKey::generate(&mut OsRng);
    let drone_public = drone_secret.public_key();
    let gs_public = gs_secret.public_key();

    write_key("drone.key", &drone_secret.to_bytes(), gs_public.as_bytes())?;
    eprintln!("Drone keypair (drone sec + gs pub) saved to drone.key");

    write_key("gs.key", &gs_secret.to_bytes(), drone_public.as_bytes())?;
    eprintln!("GS keypair (gs sec + drone pub) saved to gs.key");
    Ok(())
}

fn write_key(path: &str, secret: &[u8; 32], public: &[u8; 32]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(secret)?;
    file.write_all(public)?;
    Ok(())
}

fn print_help() {
    println!(
        r#"wfb_keygen

Generate WFB-compatible drone.key and gs.key files in the current directory.

Usage:
  wfb_keygen

The original WFB-ng tool also accepts a password and derives keys with
libsodium Argon2i. This Rust rewrite currently only generates random keys
because producing a different password-derived key would be dangerous."#
    );
}
