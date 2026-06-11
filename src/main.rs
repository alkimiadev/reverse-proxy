use reverse_proxy::cli;

fn main() {
    let args = cli::parse();

    if args.validate {
        match cli::run_validate(&args) {
            Ok(()) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let loaded = match cli::load_config(&args) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        };

        if let Err(e) =
            reverse_proxy::server::run(loaded.static_config, loaded.dynamic_config).await
        {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    });
}
