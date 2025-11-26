use clap::Parser;

/// The main structure for handling command-line arguments.
/// Clap's derive macro automatically generates the parser logic,
/// help messages, and version information from doc comments.
#[derive(Parser, Debug)]
#[command(author, version, about = "A simple 'Hello, world' program using clap 4.", long_about = None)]
pub struct Cli {
    /// An optional name to greet. If not provided, it defaults to 'World'.
    #[arg(short, long)]
    pub name: Option<String>,

    /// A flag to enable verbose output.
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn parse_cli() -> Cli {
    // Parse the arguments from the command line
    let mycli = Cli::parse();
    if mycli.name.is_some() {
        tracing::info!("Hello, {}!", mycli.name.clone().unwrap());
    }

    if mycli.verbose {
        tracing::info!("Parsed arguments: {:?}", mycli);
    }

    mycli
}
