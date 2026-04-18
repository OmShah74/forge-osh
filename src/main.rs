#![allow(warnings)]
#![allow(clippy::all)]

use clap::Parser;

use forge_agent::app::App;
use forge_agent::cli::Cli;
use forge_agent::config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_dir = config::log_dir();
    std::fs::create_dir_all(&log_dir).ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(
                    if cli.verbose {
                        "forge_agent=debug"
                    } else {
                        "forge_agent=info"
                    }
                    .parse()
                    .unwrap(),
                ),
        )
        .with_writer(std::io::stderr)
        .init();

    // Handle subcommands
    if let Some(cmd) = cli.command.clone() {
        let mut app = App::new(&cli).await?;
        return app.run_subcommand(cmd).await;
    }

    // Check if we have any provider configured
    let app = App::new(&cli).await?;
    {
        let router = app.provider_router.read().await;
        if router.available_providers().is_empty() {
            run_first_time_setup().await?;
            // Reload after setup
            drop(router);
            drop(app);
            let app = App::new(&cli).await?;
            return run_main(app, &cli).await;
        }
    }

    run_main(app, &cli).await
}

async fn run_main(app: App, cli: &Cli) -> anyhow::Result<()> {
    // Non-interactive mode
    let prompt_text = cli.prompt.join(" ");
    if !prompt_text.is_empty() {
        return app.run_once(prompt_text).await;
    }

    // Check if we're in a TTY
    use std::io::IsTerminal;
    let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    if !is_tty {
        // Pipe mode: read stdin
        use std::io::Read;
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        if !input.trim().is_empty() {
            return app.run_once(input).await;
        }
        eprintln!("No input provided.");
        return Ok(());
    }

    // Interactive TUI mode
    app.run_tui().await
}

async fn run_first_time_setup() -> anyhow::Result<()> {
    use std::io::{self, Write};

    println!();
    println!("  Welcome to forge-osh!");
    println!();
    println!("  No API keys configured. Let's get you set up.");
    println!();
    println!("  Which provider would you like to use?");
    println!("    1. Anthropic (Claude)     - Needs ANTHROPIC_API_KEY");
    println!("    2. OpenAI (GPT-4)         - Needs OPENAI_API_KEY");
    println!("    3. Groq (Fast inference)  - Needs GROQ_API_KEY");
    println!("    4. Google Gemini          - Needs GEMINI_API_KEY");
    println!("    5. Ollama (local, free)   - Needs Ollama running");
    println!("    6. Skip (configure later)");
    println!();
    print!("  Enter choice [1-6]: ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    let choice = choice.trim();

    let (provider, env_hint) = match choice {
        "1" => ("anthropic", "ANTHROPIC_API_KEY"),
        "2" => ("openai", "OPENAI_API_KEY"),
        "3" => ("groq", "GROQ_API_KEY"),
        "4" => ("gemini", "GEMINI_API_KEY"),
        "5" => {
            println!();
            println!("  Make sure Ollama is running at http://localhost:11434");
            println!("  Then restart forge-osh.");
            return Ok(());
        }
        "6" | "" => {
            println!();
            println!("  You can configure later with:");
            println!("    forge-osh config keys set <provider> <api-key>");
            println!("  Or set environment variables like ANTHROPIC_API_KEY");
            return Ok(());
        }
        _ => {
            println!("  Invalid choice.");
            return Ok(());
        }
    };

    println!();
    print!("  Enter your API key: ");
    io::stdout().flush()?;

    let mut key = String::new();
    io::stdin().read_line(&mut key)?;
    let key = key.trim();

    if key.is_empty() {
        println!("  No key provided. Set {env_hint} environment variable or run:");
        println!("    forge-osh config keys set {provider} <your-key>");
        return Ok(());
    }

    let mut store = config::keyring::KeyStore::new(&config::config_dir());
    store.set(provider, key)?;

    println!();
    println!("  API key saved for {provider}!");
    println!("  Starting forge-osh...");
    println!();

    Ok(())
}
