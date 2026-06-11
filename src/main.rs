use reverse_proxy::cli;

fn main() {
    let args = cli::parse();

    if args.validate {
        match cli::run_validate(&args) {
            Ok(()) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    match cli::load_config(&args) {
        Ok(_config) => {
            tracing::info!("reverse-proxy starting");
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}
