fn main() {
    use herdr_witch::{config, daemon, grimoire, sigil, test_cmd};
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version") => println!("herdr-witch {}", env!("CARGO_PKG_VERSION")),
        Some("daemon") => daemon::run(config::load()),
        Some("sigil") => sigil::run(&config::load()),
        Some("grimoire") => grimoire::run(&config::load()),
        Some("summon") => std::process::exit(daemon::summon::action()),
        // Hidden: a preview of the rare scenes (`sigil::familiars`).
        Some("scene") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            std::process::exit(sigil::preview(&config::load(), &args));
        }
        Some("test") => test_cmd::run(),
        _ => {
            eprintln!("usage: herdr-witch daemon|sigil|grimoire|summon|test|--version");
            std::process::exit(2);
        }
    }
}
