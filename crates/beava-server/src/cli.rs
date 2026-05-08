//! Command-line interface for the `beava` binary.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Beava v2 — single-binary real-time feature server.
#[derive(Debug, Parser)]
#[command(
    name = "beava",
    version,
    about = "Beava v2 — real-time feature server for fraud, ad-tech, and behavioral analytics",
    long_about = None,
)]
pub struct Cli {
    /// Path to YAML config file. Optional — when omitted, beava falls
    /// back to the built-in defaults (HTTP 127.0.0.1:8080, admin
    /// 127.0.0.1:8090, WAL + snapshot under ./beava-wal and
    /// ./beava-snapshots) and applies any `BEAVA_*` env-var overrides.
    /// There is no implicit `./beava.yaml` lookup — point at a YAML
    /// explicitly with `-c` / `--config` if you want one. The resolved
    /// config is logged at startup so it's clear which path was taken.
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Override the HTTP listener address. Highest precedence — wins
    /// over `--config` YAML and `BEAVA_LISTEN_ADDR` env. Locked v0
    /// default: 127.0.0.1:8080.
    #[arg(long, value_name = "ADDR", global = true)]
    pub http_addr: Option<String>,

    /// Override the TCP fast-path listener address. Highest precedence
    /// — wins over `--config` YAML and `BEAVA_TCP_HOST`/`BEAVA_TCP_PORT`
    /// env. Locked v0 default: 127.0.0.1:8081.
    #[arg(long, value_name = "ADDR", global = true)]
    pub tcp_addr: Option<String>,

    /// Override both WAL and snapshot directories under a single root:
    /// WAL lands at `<DIR>/wal`, snapshots at `<DIR>/snapshots`. Highest
    /// precedence — wins over `--config` and `BEAVA_WAL_DIR` /
    /// `BEAVA_SNAPSHOT_DIR` env. Default: `./.beava/`.
    #[arg(long, value_name = "PATH", global = true)]
    pub data_dir: Option<PathBuf>,

    /// In-memory only: no WAL writer, no snapshot, no recovery. State
    /// is lost on restart. Useful for `docker run` smoke tests, demo
    /// boots, and any "I'm just kicking the tyres" workflow that
    /// shouldn't leave bytes on disk.
    #[arg(long, global = true)]
    pub memory_only: bool,

    /// Enable POST /reset and OP_RESET (the destructive registry-wipe
    /// endpoint). Off by default; the locked v0 surface gates it on
    /// this flag (or `BEAVA_TEST_MODE=1`) so production deploys can't
    /// be reset by accident.
    #[arg(long, global = true)]
    pub test_mode: bool,

    /// Optional subcommand. When omitted, `beava` boots the server
    /// (the default behaviour). When `quickstart` is selected, beava
    /// runs the in-process 4-step demo and exits.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// `beava` subcommands. The default (no subcommand) is to boot the
/// server.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Run the magical 4-step first-touch demo.
    ///
    /// Spawns an in-process server on an ephemeral port, registers a
    /// PageView/SiteMetrics pipeline (mirrors the homepage hero), pushes
    /// 5 events, queries the global row, prints a formatted walkthrough,
    /// and writes `beava_quickstart.py` to the CWD (bridging the
    /// sandbox to a real `beava` server). 0 setup, 0 dependencies,
    /// completes in under 10 seconds.
    Quickstart {
        /// Don't write `beava_quickstart.py` to the CWD. Useful for
        /// CI / docker-exec runs where leaving a file behind is noise.
        #[arg(long)]
        no_file: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn help_contains_config_flag() {
        let mut cmd = Cli::command();
        let help = cmd.render_long_help().to_string();
        assert!(
            help.contains("--config"),
            "help must document --config: {help}"
        );
    }

    #[test]
    fn no_args_yields_none_config() {
        // `beava` (no flags) MUST parse cleanly with `config = None`
        // so main can decide whether to load `./beava.yaml`, apply env
        // overrides, or fall back to the built-in defaults. A required
        // `default_value` here forced every native install to
        // pre-create a beava.yaml, which broke the
        // `cargo install beava-server && beava` flow with a
        // "config file not found" error.
        let cli = Cli::try_parse_from(["beava"]).expect("parses with no args");
        assert_eq!(cli.config, None);
    }

    #[test]
    fn explicit_config_path_is_captured() {
        let cli = Cli::try_parse_from(["beava", "--config", "/tmp/custom.yaml"])
            .expect("parses with explicit config");
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/custom.yaml")));
    }

    /// `beava quickstart` MUST parse to `Command::Quickstart` so main.rs
    /// can dispatch into the in-process 4-step demo (1a). With no flag
    /// set, `--no-file` is `false` (we DO write the drop-file by default).
    #[test]
    fn quickstart_subcommand_parses_with_default_flags() {
        let cli = Cli::try_parse_from(["beava", "quickstart"]).expect("parses `beava quickstart`");
        assert_eq!(
            cli.command,
            Some(Command::Quickstart { no_file: false }),
            "expected Command::Quickstart {{ no_file: false }}"
        );
    }

    /// `beava quickstart --no-file` flips `no_file` to `true` so CI /
    /// docker-exec runs can opt out of leaving `beava_quickstart.py`
    /// behind in the working directory.
    #[test]
    fn quickstart_subcommand_no_file_flag_parses() {
        let cli = Cli::try_parse_from(["beava", "quickstart", "--no-file"])
            .expect("parses `beava quickstart --no-file`");
        assert_eq!(
            cli.command,
            Some(Command::Quickstart { no_file: true }),
            "expected Command::Quickstart {{ no_file: true }}"
        );
    }

    // ─── Locked v0 Server CLI flags (F5) ──────────────────────────────
    // The README documents `--http-addr`, `--tcp-addr`, `--data-dir`,
    // `--memory-only`, and `--test-mode` as the canonical OSS-launch
    // surface. These tests pin the clap recognition of each so a future
    // refactor can't silently drop a flag that the docs promise.

    #[test]
    fn http_addr_flag_is_recognized() {
        let result = Cli::try_parse_from(["beava", "--http-addr", "127.0.0.1:9000"]);
        assert!(
            result.is_ok(),
            "--http-addr must be a recognized flag; got {:?}",
            result.err()
        );
    }

    #[test]
    fn tcp_addr_flag_is_recognized() {
        let result = Cli::try_parse_from(["beava", "--tcp-addr", "127.0.0.1:9001"]);
        assert!(
            result.is_ok(),
            "--tcp-addr must be a recognized flag; got {:?}",
            result.err()
        );
    }

    #[test]
    fn data_dir_flag_is_recognized() {
        let result = Cli::try_parse_from(["beava", "--data-dir", "/tmp/beava-data"]);
        assert!(
            result.is_ok(),
            "--data-dir must be a recognized flag; got {:?}",
            result.err()
        );
    }

    #[test]
    fn memory_only_flag_is_recognized() {
        let result = Cli::try_parse_from(["beava", "--memory-only"]);
        assert!(
            result.is_ok(),
            "--memory-only must be a recognized flag; got {:?}",
            result.err()
        );
    }

    #[test]
    fn test_mode_flag_is_recognized() {
        let result = Cli::try_parse_from(["beava", "--test-mode"]);
        assert!(
            result.is_ok(),
            "--test-mode must be a recognized flag; got {:?}",
            result.err()
        );
    }

    // Value assertions for the F5 flags. These complement the recognition
    // tests above — once clap accepts each flag, pin the parsed value so
    // a future refactor that drops a field type can't go unnoticed.

    #[test]
    fn http_addr_flag_captures_value() {
        let cli = Cli::try_parse_from(["beava", "--http-addr", "0.0.0.0:9090"]).unwrap();
        assert_eq!(cli.http_addr.as_deref(), Some("0.0.0.0:9090"));
    }

    #[test]
    fn tcp_addr_flag_captures_value() {
        let cli = Cli::try_parse_from(["beava", "--tcp-addr", "0.0.0.0:9091"]).unwrap();
        assert_eq!(cli.tcp_addr.as_deref(), Some("0.0.0.0:9091"));
    }

    #[test]
    fn data_dir_flag_captures_value() {
        let cli = Cli::try_parse_from(["beava", "--data-dir", "/var/lib/beava"]).unwrap();
        assert_eq!(cli.data_dir, Some(PathBuf::from("/var/lib/beava")));
    }

    #[test]
    fn memory_only_flag_captures_true() {
        let cli = Cli::try_parse_from(["beava", "--memory-only"]).unwrap();
        assert!(cli.memory_only);
    }

    #[test]
    fn memory_only_flag_default_false() {
        let cli = Cli::try_parse_from(["beava"]).unwrap();
        assert!(!cli.memory_only);
    }

    #[test]
    fn test_mode_flag_captures_true() {
        let cli = Cli::try_parse_from(["beava", "--test-mode"]).unwrap();
        assert!(cli.test_mode);
    }

    #[test]
    fn test_mode_flag_default_false() {
        let cli = Cli::try_parse_from(["beava"]).unwrap();
        assert!(!cli.test_mode);
    }
}
