use std::{env, io};

use media_control::{AppResult, artwork, credit, model};

fn main() {
    if let Err(error) = run() {
        eprintln!("media-control: {error}");
        std::process::exit(2);
    }
}

fn run() -> AppResult<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("model") => model::serve(),
        Some("credit") => credit::print_credit(),
        Some("artwork") => {
            let url = args
                .next()
                .ok_or_else(|| io::Error::other("track URL is missing"))?;
            artwork::print_path(&url)
        }
        None | Some("help" | "--help" | "-h") => {
            println!(
                "media-control\n\n\
                 Usage:\n  \
                 media-control model (JSONL native snapshots on stdin)\n  \
                 media-control credit (JSON track metadata on stdin)\n  \
                 media-control artwork <xesam-url|path>"
            );
            Ok(())
        }
        Some(command) => Err(io::Error::other(format!("unknown command {command:?}")).into()),
    }
}
