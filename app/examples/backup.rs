//! The backup bundle against a live state — the desk's Me page without the
//! window. `export` writes the phone-compatible bundle; `import` restores it
//! into whatever state DUCAT_DESK_STATE names (a fresh directory to prove a
//! restore, the same one to prove idempotence). Markers: BK_OK, BK_FAIL.
//!
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example backup -- export <file> <passphrase>
//!   DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example backup -- import <file> <passphrase>

use std::path::Path;

use ducat_app::App;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = App::open_default().expect("BK_FAIL open");
    eprintln!("state: {}", app.root().display());
    match args.first().map(String::as_str) {
        Some("export") => {
            let file = args.get(1).expect("BK_FAIL export <file> <passphrase>");
            let pass = args.get(2).expect("BK_FAIL export <file> <passphrase>");
            let n = app.export_backup_to(Path::new(file), pass).expect("BK_FAIL export");
            println!("BK_OK exported {n} bytes, {} contact(s), {} persona(s)", app.contacts().len(), app.personas().map(|p| p.len()).unwrap_or(0));
        }
        Some("import") => {
            let file = args.get(1).expect("BK_FAIL import <file> <passphrase>");
            let pass = args.get(2).expect("BK_FAIL import <file> <passphrase>");
            let r = app.import_backup_from(Path::new(file), pass).expect("BK_FAIL import");
            println!(
                "BK_OK restored {} contact(s), {} persona(s), wallet from height {}; now {} contact(s), name {:?}, address {}",
                r.contacts,
                r.personas,
                r.restore_height,
                app.contacts().len(),
                app.my_name(None).ok().flatten().unwrap_or_default(),
                app.wallet_address().as_deref().map(|a| &a[..12]).unwrap_or("-")
            );
        }
        _ => panic!("BK_FAIL usage: export <file> <passphrase> | import <file> <passphrase>"),
    }
}
