use std::path::{Path, PathBuf};

use anyhow::Context;
use libtest_mimic::{Arguments, Failed, Trial};

fn main() -> anyhow::Result<()> {
    let args = Arguments::from_args();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/regressions");
    let mut paths = Vec::new();
    collect_fixtures(&root, &mut paths)?;
    paths.sort();

    let tests = paths
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(&root)
                .expect("fixture under root")
                .to_string_lossy()
                .replace('\\', "/");
            Trial::test(name, move || {
                replay(&path).map_err(|error| Failed::from(format!("{error:#}")))
            })
        })
        .collect();

    libtest_mimic::run(&args, tests).exit();
}

fn collect_fixtures(dir: &Path, paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading fixture directory {}", dir.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_fixtures(&path, paths)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "wasm") {
            paths.push(path);
        }
    }
    Ok(())
}

fn replay(path: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading regression fixture {}", path.display()))?;

    // Malformed inputs may return errors; a panic fails this fixture's trial.
    if let Ok(module) = wasmrs::parse_module(&bytes) {
        let _ = wasmrs::validate_module(&module);
    }
    Ok(())
}
