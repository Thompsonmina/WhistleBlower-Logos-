use anyhow::{anyhow, Context, Result};
use borsh::BorshDeserialize;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use toml::Value as TomlValue;
use tracing::{debug, warn};

use super::types::Registry;
use crate::batch::PendingEntry;
use crate::config::RegistryConfig;

/// Reads and writes on-chain registry state by shelling out to `lgs wallet` and
/// the chronicle-registry CLI binary (the auto-generated SPEL client).
///
/// Invokes the prebuilt binary at `<registry_dir>/target/debug/chronicle_registry_cli`
/// directly to avoid `cargo run` rebuild overhead.  All paths are resolved
/// to absolutes at construction so child processes can run from any cwd.
#[derive(Clone)]
pub struct RegistryClient {
    /// Directory containing spel.toml (the chronicle-registry root), absolute.
    registry_dir: PathBuf,
    /// Prebuilt CLI binary path, absolute.
    registry_cli_bin: PathBuf,
    /// IDL JSON path (absolute) — the spel CLI needs this whenever any
    /// option flag is passed (it bypasses spel.toml auto-discovery in that mode).
    idl_path: PathBuf,
    /// Public signer ID — needed for dry-run PDA derivation.
    signer_account_id: String,
    /// Wallet home dir, absolute.  Forwarded to every `lgs` invocation.
    wallet_home: PathBuf,
    /// 64-char hex program ID — passed to spel CLI via `-p` so the tool
    /// is pinned to a specific deployed registry and never re-derives it
    /// from a local binary.
    program_id: String,
}

#[derive(Deserialize)]
struct AccountGetOutput {
    data: Option<String>, // hex-encoded account bytes
}

impl RegistryClient {
    pub fn new(cfg: &RegistryConfig) -> Result<Self> {
        let spel_toml = cfg
            .spel_toml
            .canonicalize()
            .with_context(|| format!("resolving registry.spel_toml: {}", cfg.spel_toml.display()))?;
        let registry_dir = spel_toml
            .parent()
            .ok_or_else(|| anyhow!("registry.spel_toml has no parent dir: {}", spel_toml.display()))?
            .to_path_buf();
        let registry_cli_bin = registry_dir.join("target/debug/chronicle_registry_cli");
        if !registry_cli_bin.exists() {
            anyhow::bail!(
                "chronicle_registry_cli binary not found at {} — run `make build` in chronicle-registry first",
                registry_cli_bin.display()
            );
        }
        let wallet_home = cfg
            .wallet_home
            .canonicalize()
            .with_context(|| format!("resolving registry.wallet_home: {}", cfg.wallet_home.display()))?;
        let program_id = cfg.program_id.trim().to_string();
        anyhow::ensure!(
            program_id.len() == 64 && program_id.chars().all(|c| c.is_ascii_hexdigit()),
            "registry.program_id must be 64 lowercase hex chars (got {} chars: {:?})",
            program_id.len(),
            program_id
        );

        // Parse spel.toml's [program] idl entry so we can pass --idl explicitly.
        // The CLI requires this whenever any other flag is passed.
        let spel_raw = std::fs::read_to_string(&spel_toml)
            .with_context(|| format!("reading {}", spel_toml.display()))?;
        let spel_doc: TomlValue = toml::from_str(&spel_raw)
            .with_context(|| format!("parsing {}", spel_toml.display()))?;
        let idl_rel = spel_doc
            .get("program")
            .and_then(|p| p.get("idl"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("{} missing [program].idl", spel_toml.display()))?;
        let idl_path = registry_dir.join(idl_rel).canonicalize().with_context(|| {
            format!("resolving IDL path {} (from {})", idl_rel, spel_toml.display())
        })?;

        Ok(Self {
            registry_dir,
            registry_cli_bin,
            idl_path,
            signer_account_id: cfg.signer_account_id.clone(),
            wallet_home,
            program_id,
        })
    }

    fn lgs(&self) -> Command {
        let mut cmd = Command::new("lgs");
        cmd.current_dir(&self.registry_dir)
            .env("NSSA_WALLET_HOME_DIR", &self.wallet_home);
        cmd
    }

    /// Build a spel CLI invocation: `-i <idl> -p <hex> [opts...] -- <cmd> [args...]`.
    ///
    /// `opts` are spel-level OPTIONS that must appear *before* the `--`
    /// separator (e.g. `--dry-run=text`).  `args` are the command name
    /// plus its instruction-level args and go *after* the separator.
    ///
    /// Once any option is passed the spel CLI stops auto-discovering
    /// spel.toml, so we always supply both `-i` and `-p` and the `--`.
    fn registry_cli(&self, opts: &[&str], args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.registry_cli_bin);
        cmd.arg("-i")
            .arg(&self.idl_path)
            .arg("-p")
            .arg(&self.program_id);
        for opt in opts {
            cmd.arg(opt);
        }
        cmd.arg("--")
            .args(args)
            .current_dir(&self.registry_dir)
            .env("NSSA_WALLET_HOME_DIR", &self.wallet_home);
        cmd
    }

    /// Derive the registry PDA address via a dry-run of `init-registry`.
    /// The CLI prints a line like:  `PDA registry → <ADDRESS> [writable]`
    pub fn derive_pda(&self) -> Result<String> {
        let output = self
            .registry_cli(
                &["--dry-run=text"],
                &["init-registry", "--anchorer", &self.signer_account_id],
            )
            .output()
            .context("running chronicle_registry_cli init-registry --dry-run")?;

        // Search both stdout and stderr — the dry-run text output may go to either.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        for line in stdout.lines().chain(stderr.lines()) {
            // Expected:  "PDA registry → <ADDR> [writable]" (or "->" on ASCII-only terminals)
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("PDA registry") {
                // Strip the arrow ("→" or "->") and any whitespace
                let after_arrow = rest
                    .trim_start()
                    .trim_start_matches('\u{2192}')
                    .trim_start_matches("->")
                    .trim_start();
                // Address ends at first whitespace
                if let Some(addr) = after_arrow.split_whitespace().next() {
                    return Ok(addr.to_string());
                }
            }
        }
        anyhow::bail!(
            "could not find 'PDA registry <ADDRESS>' in dry-run output\nstdout: {}\nstderr: {}",
            stdout,
            stderr
        );
    }

    /// Fetch raw account bytes for an address via `lgs wallet account get --raw`.
    /// Returns None if the account does not exist.
    pub fn fetch_account_bytes(&self, pda: &str) -> Result<Option<Vec<u8>>> {
        let output = self
            .lgs()
            .args(["wallet", "--", "account", "get", "--account-id"])
            .arg(format!("Public/{}", pda))
            .arg("--raw")
            .output()
            .context("running lgs wallet account get")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            let combined = format!("{}{}", stdout, stderr);
            if combined.contains("not found") || combined.contains("does not exist") {
                return Ok(None);
            }
            anyhow::bail!("lgs wallet account get failed: {}{}", stdout, stderr);
        }

        // lgs prints a JSON line containing the hex-encoded `data` field.
        for line in stdout.lines() {
            let line = line.trim();
            if !line.starts_with('{') {
                continue;
            }
            if let Ok(obj) = serde_json::from_str::<AccountGetOutput>(line) {
                if let Some(hex_data) = obj.data {
                    let bytes = hex::decode(&hex_data).context("hex-decode account data")?;
                    return Ok(Some(bytes));
                }
            }
        }
        Ok(None)
    }

    /// Fetch + decode the registry account. Returns None if the PDA does not exist
    /// (i.e. init_registry has not been run yet).
    pub fn fetch_registry(&self) -> Result<Option<Registry>> {
        let pda = self.derive_pda()?;
        debug!(pda = %pda, "derived registry PDA");
        match self.fetch_account_bytes(&pda)? {
            None => Ok(None),
            Some(bytes) => {
                let reg = Registry::try_from_slice(&bytes)
                    .context("Borsh-decode Registry from account data")?;
                debug!(entries = reg.entries.len(), "registry decoded");
                Ok(Some(reg))
            }
        }
    }

    /// Build a HashSet of already-anchored CIDs from on-chain state.
    /// Used at startup to seed the dedup set. Logs a warning (and returns empty)
    /// if the registry PDA does not exist yet.
    pub fn anchored_cid_set(&self) -> Result<HashSet<String>> {
        match self.fetch_registry()? {
            Some(reg) => Ok(reg.entries.into_keys().collect()),
            None => {
                warn!("registry PDA not found — run `batch-anchor init` first; starting with empty dedup set");
                Ok(HashSet::new())
            }
        }
    }

    /// Submit `init_registry`.
    ///
    /// Returns Ok(true) if newly initialised, Ok(false) if the registry PDA
    /// already exists (no-op success), Err otherwise.
    ///
    /// Note: we pre-check existence by fetching the PDA rather than submitting
    /// and waiting for a 1002 rejection.  The chronicle_registry_cli currently
    /// hangs on the "waiting for confirmation" step when the sequencer rejects
    /// a duplicate init, so we avoid that path entirely.
    pub fn init_registry(&self) -> Result<bool> {
        let pda = self.derive_pda()?;
        if self.fetch_account_bytes(&pda)?.is_some() {
            debug!(pda = %pda, "init-registry: PDA already exists (no-op)");
            return Ok(false);
        }

        let output = self
            .registry_cli(
                &[],
                &["init-registry", "--anchorer", &self.signer_account_id],
            )
            .output()
            .context("running chronicle_registry_cli init-registry")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            debug!(stdout = %stdout, "init-registry succeeded");
            return Ok(true);
        }

        anyhow::bail!("init-registry failed: {}{}", stdout, stderr);
    }

    /// Submit `index_batch` for the given pending entries.
    ///
    /// Entries are formatted as three parallel comma-separated lists matching the
    /// chronicle-registry IDL: cids (UTF-8 strings, no commas inside), metadata
    /// hashes (64-char lowercase hex, one per entry), and anchor timestamps
    /// (decimal u32 unix-seconds).  The on-chain program enforces equal length
    /// and rejects empty / oversized batches; we still pre-check both client-side
    /// to fail fast.
    pub fn index_batch(&self, entries: &[PendingEntry]) -> Result<()> {
        anyhow::ensure!(!entries.is_empty(), "index_batch: empty batch");
        anyhow::ensure!(
            entries.len() <= crate::batch::MAX_BATCH,
            "index_batch: batch of {} exceeds MAX_BATCH={}",
            entries.len(),
            crate::batch::MAX_BATCH
        );

        let cids = entries
            .iter()
            .map(|e| e.cid.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let hashes = entries
            .iter()
            .map(|e| hex::encode(e.metadata_hash))
            .collect::<Vec<_>>()
            .join(",");
        // Registry IDL takes anchor_timestamps as Vec<u32> — safe until 2106.
        let timestamps = entries
            .iter()
            .map(|e| (e.timestamp as u32).to_string())
            .collect::<Vec<_>>()
            .join(",");

        debug!(count = entries.len(), "submitting index_batch");
        let output = self
            .registry_cli(
                &[],
                &[
                    "index-batch",
                    "--anchorer",
                    &self.signer_account_id,
                    "--cids",
                    &cids,
                    "--metadata-hashes",
                    &hashes,
                    "--anchor-timestamps",
                    &timestamps,
                ],
            )
            .output()
            .context("running chronicle_registry_cli index-batch")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            anyhow::bail!(
                "index-batch failed (exit {}): {}{}",
                output.status,
                stdout,
                stderr
            );
        }
        debug!(stdout = %stdout, "index-batch succeeded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // StateReader::new performs filesystem canonicalisation + binary-exists check,
    // so it can only be exercised end-to-end against a real registry checkout.
    // Pure-parser tests follow below.

    /// Parse a line in the shape printed by chronicle_registry_cli --dry-run=text.
    fn parse_pda_line(line: &str) -> Option<String> {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix("PDA registry")?;
        let after_arrow = rest
            .trim_start()
            .trim_start_matches('\u{2192}')
            .trim_start_matches("->")
            .trim_start();
        after_arrow.split_whitespace().next().map(str::to_string)
    }

    #[test]
    fn parses_pda_line_with_unicode_arrow() {
        assert_eq!(
            parse_pda_line("  PDA registry → HwzZhhEXX6dT6PRjP4ufNRuR5hnEHxv1PXLFn7JHhtgs [writable]"),
            Some("HwzZhhEXX6dT6PRjP4ufNRuR5hnEHxv1PXLFn7JHhtgs".into())
        );
    }

    #[test]
    fn parses_pda_line_with_ascii_arrow() {
        assert_eq!(
            parse_pda_line("PDA registry -> AbCdEf123 [writable]"),
            Some("AbCdEf123".into())
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        assert_eq!(parse_pda_line("Program ID: ..."), None);
        assert_eq!(parse_pda_line("Accounts:"), None);
    }
}
