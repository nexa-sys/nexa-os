//! nvmctl - NVM Command Line Management Tool
//!
//! Enterprise CLI for managing NVM hypervisor platform.
//! Usage: nvmctl [OPTIONS] <COMMAND> [ARGS]

use std::process::ExitCode;

mod cli_impl {
    pub use nvm::cli::*;
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        return ExitCode::from(1);
    }

    let config = cli_impl::load_config();
    let formatter = cli_impl::output::OutputFormatter::new(config.output_format);

    // Parse global options and command
    let mut i = 1;
    let mut output_format = config.output_format;
    
    while i < args.len() && args[i].starts_with('-') {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("nvmctl {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output_format = args[i].parse().unwrap_or(cli_impl::OutputFormat::Table);
                }
            }
            "-q" | "--quiet" => {
                // Quiet mode
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                return ExitCode::from(1);
            }
        }
        i += 1;
    }

    if i >= args.len() {
        print_usage();
        return ExitCode::from(1);
    }

    let command = &args[i];
    let cmd_args: Vec<&str> = args[i + 1..].iter().map(|s| s.as_str()).collect();
    let formatter = cli_impl::output::OutputFormatter::new(output_format);

    let result = match command.as_str() {
        "vm" => handle_vm(&cmd_args, &formatter),
        "storage" => handle_storage(&cmd_args, &formatter),
        "network" => handle_network(&cmd_args, &formatter),
        "cluster" => handle_cluster(&cmd_args, &formatter),
        "backup" => handle_backup(&cmd_args, &formatter),
        "user" => handle_user(&cmd_args, &formatter),
        "system" => handle_system(&cmd_args, &formatter),
        "config" => handle_config(&cmd_args, &formatter),
        "shell" => {
            let mut shell = cli_impl::shell::Shell::new();
            shell.run().map(|_| ())
        }
        "help" => {
            print_usage();
            Ok(())
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            print_usage();
            Err(cli_impl::CliError::InvalidArg(format!("Unknown command: {}", command)))
        }
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            formatter.error(&e.to_string());
            ExitCode::from(1)
        }
    }
}

fn print_usage() {
    println!("nvmctl - NVM Hypervisor Management CLI");
    println!();
    println!("USAGE:");
    println!("    nvmctl [OPTIONS] <COMMAND> [ARGS]");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help             Show this help message");
    println!("    -V, --version          Show version information");
    println!("    -o, --output <FORMAT>  Output format (table|json|yaml|csv)");
    println!("    -q, --quiet            Suppress non-essential output");
    println!();
    println!("COMMANDS:");
    println!("    vm         Virtual machine management");
    println!("    storage    Storage pool and volume management");
    println!("    network    Network management");
    println!("    cluster    Cluster and node management");
    println!("    backup     Backup and restore operations");
    println!("    user       User and access management");
    println!("    system     System information and configuration");
    println!("    config     CLI configuration");
    println!("    shell      Interactive shell mode");
    println!("    help       Show this help message");
    println!();
    println!("EXAMPLES:");
    println!("    nvmctl vm list");
    println!("    nvmctl vm create --name myvm --memory 4096 --vcpus 2");
    println!("    nvmctl vm start vm-001");
    println!("    nvmctl storage pools");
    println!("    nvmctl cluster status");
    println!("    nvmctl shell");
}

fn handle_vm(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("VM Commands:");
        println!("    list              List all virtual machines");
        println!("    info <id>         Show VM details");
        println!("    create [opts]     Create new VM");
        println!("    start <id>        Start VM");
        println!("    stop <id>         Stop VM");
        println!("    restart <id>      Restart VM");
        println!("    delete <id>       Delete VM");
        println!("    snapshot <id>     Create snapshot");
        println!("    clone <id>        Clone VM");
        println!("    migrate <id>      Migrate VM");
        println!("    console <id>      Open console");
        return Ok(());
    }

    match args[0] {
        "list" | "ls" => {
            let vms = cli_impl::commands::vm::list(cli_impl::OutputFormat::Table, false, None)?;
            fmt.print(&vms)?;
        }
        "info" if args.len() > 1 => {
            let vm = cli_impl::commands::vm::info(args[1])?;
            fmt.print(&vm)?;
        }
        "start" if args.len() > 1 => {
            cli_impl::commands::vm::start(args[1])?;
            fmt.success(&format!("VM {} started", args[1]));
        }
        "stop" if args.len() > 1 => {
            let force = args.iter().any(|a| *a == "--force" || *a == "-f");
            cli_impl::commands::vm::stop(args[1], force)?;
            fmt.success(&format!("VM {} stopped", args[1]));
        }
        "restart" if args.len() > 1 => {
            cli_impl::commands::vm::restart(args[1])?;
            fmt.success(&format!("VM {} restarted", args[1]));
        }
        "delete" if args.len() > 1 => {
            let force = args.iter().any(|a| *a == "--force" || *a == "-f");
            cli_impl::commands::vm::delete(args[1], force)?;
            fmt.success(&format!("VM {} deleted", args[1]));
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid vm subcommand".into()));
        }
    }
    Ok(())
}

fn handle_storage(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("Storage Commands:");
        println!("    pools             List storage pools");
        println!("    volumes           List volumes");
        println!("    create [opts]     Create volume");
        return Ok(());
    }

    match args[0] {
        "pools" => {
            let pools = cli_impl::commands::storage::list_pools()?;
            fmt.print(&pools)?;
        }
        "volumes" | "vols" => {
            let vols = cli_impl::commands::storage::list_volumes(None)?;
            fmt.print(&vols)?;
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid storage subcommand".into()));
        }
    }
    Ok(())
}

fn handle_network(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("Network Commands:");
        println!("    list              List networks");
        println!("    create [opts]     Create network");
        return Ok(());
    }

    match args[0] {
        "list" | "ls" => {
            let nets = cli_impl::commands::network::list()?;
            fmt.print(&nets)?;
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid network subcommand".into()));
        }
    }
    Ok(())
}

fn handle_cluster(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("Cluster Commands:");
        println!("    status            Show cluster status");
        println!("    nodes             List cluster nodes");
        println!("    join [opts]       Join cluster");
        println!("    leave             Leave cluster");
        return Ok(());
    }

    match args[0] {
        "status" => {
            let status = cli_impl::commands::cluster::status()?;
            fmt.print(&status)?;
        }
        "nodes" => {
            let nodes = cli_impl::commands::cluster::list_nodes()?;
            fmt.print(&nodes)?;
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid cluster subcommand".into()));
        }
    }
    Ok(())
}

fn handle_backup(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("Backup Commands:");
        println!("    list              List backups");
        println!("    create [opts]     Create backup");
        println!("    restore <id>      Restore backup");
        return Ok(());
    }

    match args[0] {
        "list" | "ls" => {
            let backups = cli_impl::commands::backup::list(None)?;
            fmt.print(&backups)?;
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid backup subcommand".into()));
        }
    }
    Ok(())
}

fn handle_user(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("User Commands:");
        println!("    list              List users");
        println!("    create [opts]     Create user");
        println!("    passwd <user>     Change password");
        return Ok(());
    }

    match args[0] {
        "list" | "ls" => {
            let users = cli_impl::commands::user::list()?;
            fmt.print(&users)?;
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid user subcommand".into()));
        }
    }
    Ok(())
}

fn handle_system(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("System Commands:");
        println!("    info              Show system information");
        println!("    license           Show license information");
        println!("    update            Check for updates");
        return Ok(());
    }

    match args[0] {
        "info" => {
            let info = cli_impl::commands::system::info()?;
            fmt.print(&info)?;
        }
        "license" => {
            let lic = cli_impl::commands::system::license()?;
            fmt.print(&lic)?;
        }
        "update" => {
            let upd = cli_impl::commands::system::update_check()?;
            fmt.print(&upd)?;
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid system subcommand".into()));
        }
    }
    Ok(())
}

fn handle_config(args: &[&str], fmt: &cli_impl::output::OutputFormatter) -> cli_impl::CliResult<()> {
    if args.is_empty() {
        println!("Config Commands:");
        println!("    show              Show current configuration");
        println!("    set <key> <val>   Set configuration value");
        println!("    setup             Run setup wizard");
        println!("    login             Login and save credentials");
        println!("    logout            Clear saved credentials");
        return Ok(());
    }

    match args[0] {
        "show" => {
            let cfg = cli_impl::config::show()?;
            println!("API URL:      {}", cfg.api_url);
            println!("Output:       {:?}", cfg.output_format);
            println!("Verify TLS:   {}", cfg.verify_tls);
            println!("Timeout:      {}s", cfg.timeout);
            println!("Token:        {}", if cfg.api_token.is_some() { "(set)" } else { "(not set)" });
        }
        "set" if args.len() > 2 => {
            cli_impl::config::configure(args[1], args[2])?;
            fmt.success(&format!("Configuration {} updated", args[1]));
        }
        "setup" => {
            cli_impl::config::setup()?;
        }
        "login" => {
            // Simplified login
            fmt.info("Use 'nvmctl config setup' for interactive login");
        }
        "logout" => {
            cli_impl::config::logout()?;
            fmt.success("Credentials cleared");
        }
        _ => {
            return Err(cli_impl::CliError::InvalidArg("Invalid config subcommand".into()));
        }
    }
    Ok(())
}
