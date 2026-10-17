//! `recall fixtures gen|verify` — sha256 manifest for the local-only real
//! fixtures (`fixtures/real/`), used as a drift sentinel:
//! after a VS Code upgrade, refresh the copies and diff the manifest to see
//! whether the on-disk format/content structure changed.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Args, Debug)]
pub struct FixturesArgs {
    #[command(subcommand)]
    pub cmd: FixturesCmd,
}

#[derive(Subcommand, Debug)]
pub enum FixturesCmd {
    /// Write manifest.json for all fixture files in the directory.
    Gen {
        #[arg(long, default_value = "fixtures/real")]
        dir: PathBuf,
    },
    /// Verify files against manifest.json (missing/size/hash mismatch = error).
    Verify {
        #[arg(long, default_value = "fixtures/real")]
        dir: PathBuf,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct Manifest {
    version: u32,
    files: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Entry {
    path: String,
    size: u64,
    sha256: String,
}

const MANIFEST_NAME: &str = "manifest.json";
const SKIP: [&str; 2] = ["README.md", MANIFEST_NAME];

pub fn run(args: FixturesArgs) -> Result<()> {
    match args.cmd {
        FixturesCmd::Gen { dir } => generate_manifest(&dir),
        FixturesCmd::Verify { dir } => verify(&dir),
    }
}

fn fixture_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        out.push(entry.path());
    }
    out.sort();
    Ok(out)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        use std::io::Read;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn generate_manifest(dir: &Path) -> Result<()> {
    let files = fixture_files(dir)?;
    if files.is_empty() {
        bail!("no fixture files found in {}", dir.display());
    }
    let mut entries = Vec::with_capacity(files.len());
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let size = std::fs::metadata(path)?.len();
        let sha256 = sha256_file(path)?;
        eprintln!("hashed {name} ({size} bytes)");
        entries.push(Entry {
            path: name,
            size,
            sha256,
        });
    }
    let manifest = Manifest { version: 1, files: entries };
    let text = serde_json::to_string_pretty(&manifest)? + "\n";
    let out_path = dir.join(MANIFEST_NAME);
    std::fs::write(&out_path, text)?;
    println!(
        "wrote {} ({} files)",
        out_path.display(),
        manifest.files.len()
    );
    Ok(())
}

fn verify(dir: &Path) -> Result<()> {
    let manifest_path = dir.join(MANIFEST_NAME);
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_str(&text)?;

    let mut problems = 0usize;
    let mut ok = 0usize;
    let mut manifest_names = std::collections::BTreeSet::new();
    for entry in &manifest.files {
        manifest_names.insert(entry.path.clone());
        let path = dir.join(&entry.path);
        if !path.is_file() {
            println!("MISSING  {}", entry.path);
            problems += 1;
            continue;
        }
        let size = std::fs::metadata(&path)?.len();
        if size != entry.size {
            println!(
                "SIZE     {} (manifest {} != disk {})",
                entry.path, entry.size, size
            );
            problems += 1;
            continue;
        }
        let sha = sha256_file(&path)?;
        if sha != entry.sha256 {
            println!(
                "SHA256   {} (manifest {} != disk {})",
                entry.path, entry.sha256, sha
            );
            problems += 1;
            continue;
        }
        ok += 1;
    }

    // Files on disk not tracked by the manifest (informational).
    for path in fixture_files(dir)? {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !manifest_names.contains(&name) {
            println!("UNTRACKED {}", name);
        }
    }

    if problems > 0 {
        bail!("manifest verify FAILED: {problems} problem(s), {ok} ok");
    }
    println!("manifest verify OK: {ok} files match");
    Ok(())
}
