use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcCommand;

use chrono::Duration;
use chrono::NaiveDate;
use clap::Parser;
use clap::Subcommand;
use fs2::FileExt;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "renogymon-archiver-puller")]
#[command(about = "Pull Renogy Parquet archives from the RPi4 over Tailscale")]
struct Args {
    /// rsync source: <user>@<host>:<path> (path is relative to the rrsync root, e.g. ./)
    #[arg(long, env = "ARCHIVER_REMOTE")]
    remote: Option<String>,

    /// Local archive directory
    #[arg(long, env = "ARCHIVER_DEST", default_value = "/var/lib/renogy-archive")]
    dest: PathBuf,

    /// SSH private key
    #[arg(
        long,
        env = "ARCHIVER_SSH_KEY",
        default_value = "/var/lib/renogymon-archiver-puller/id_ed25519"
    )]
    ssh_key: PathBuf,

    /// Lock file guarding against overlapping runs
    #[arg(long, default_value = "/var/lib/renogymon-archiver-puller/.lock")]
    lock_file: PathBuf,

    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pull staged files from the Pi and delete-on-success
    Pull,
    /// Audit the local archive dir for completeness / gaps
    Status,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let default_filter = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)),
        )
        .init();

    match args.command {
        Command::Pull => pull(&args),
        Command::Status => status(&args.dest),
    }
}

fn pull(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let remote = args.remote.clone().ok_or(
        "ARCHIVER_REMOTE not set (pass --remote or set it in /etc/default/renogymon-archiver-puller)",
    )?;
    std::fs::create_dir_all(&args.dest)?;

    if let Some(parent) = args.lock_file.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&args.lock_file)?;
    if lock.try_lock_exclusive().is_err() {
        tracing::warn!("Another pull is already running; exiting");
        return Ok(());
    }

    let ssh = format!(
        "ssh -i {} -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        args.ssh_key.display()
    );
    let dest_arg = format!("{}/", args.dest.display());

    tracing::info!("Pulling {remote} -> {dest_arg}");
    let status = ProcCommand::new("rsync")
        .arg("-a")
        .arg("--remove-source-files")
        .arg("--partial")
        .arg("-e")
        .arg(&ssh)
        .arg(&remote)
        .arg(&dest_arg)
        .status()?;

    if !status.success() {
        return Err(format!("rsync exited unsuccessfully: {status}").into());
    }
    tracing::info!("Pull complete");
    Ok(())
}

fn parse_day(name: &str) -> Option<NaiveDate> {
    let stem = name.strip_prefix("renogy_")?.strip_suffix(".parquet")?;
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
}

fn status(dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut days: Vec<NaiveDate> = Vec::new();
    match std::fs::read_dir(dest) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if let Some(d) = parse_day(&entry.file_name().to_string_lossy()) {
                    days.push(d);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("archive dir: {} (does not exist yet)", dest.display());
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    }
    days.sort();
    days.dedup();

    println!("archive dir: {}", dest.display());
    let (Some(&first), Some(&last)) = (days.first(), days.last()) else {
        println!("files: 0 (empty)");
        return Ok(());
    };

    let gaps = missing_ranges(&days);
    let missing: i64 = gaps.iter().map(|&(a, b)| (b - a).num_days() + 1).sum();

    println!("files: {}", days.len());
    println!("range: {first} .. {last}");
    if gaps.is_empty() {
        println!("gaps:  none - contiguous");
    } else {
        println!(
            "gaps:  {missing} missing day(s) in {} range(s):",
            gaps.len()
        );
        for (a, b) in gaps {
            if a == b {
                println!("  {a}");
            } else {
                println!("  {a} .. {b} ({} days)", (b - a).num_days() + 1);
            }
        }
    }
    Ok(())
}

/// Inclusive ranges of calendar days absent between consecutive entries of
/// `days`, which must be sorted and deduplicated.
fn missing_ranges(days: &[NaiveDate]) -> Vec<(NaiveDate, NaiveDate)> {
    days.windows(2)
        .filter(|w| w[1] - w[0] > Duration::days(1))
        .map(|w| (w[0] + Duration::days(1), w[1] - Duration::days(1)))
        .collect()
}
