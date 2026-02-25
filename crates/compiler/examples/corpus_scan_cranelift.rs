use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use codegen::backends::cranelift::{
    CraneliftOptions, compile_fir_to_cranelift_jit, diagnose_cranelift_compute_subset_gap,
};
use compiler::{Compiler, SignalFirLane};

fn main() {
    let filters: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new("tests/corpus");
    let mut files: Vec<PathBuf> = fs::read_dir(root)
        .expect("read tests/corpus")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dsp"))
        .filter(|p| {
            if filters.is_empty() {
                true
            } else {
                let s = p.to_string_lossy();
                filters.iter().any(|f| s.contains(f))
            }
        })
        .collect();
    files.sort();

    let compiler = Compiler::new();
    let mut lowered_ok: Vec<PathBuf> = Vec::new();
    let mut stub_ok: Vec<PathBuf> = Vec::new();
    let mut stub_examples: Vec<(PathBuf, String)> = Vec::new();
    let mut stub_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors: Vec<(PathBuf, String)> = Vec::new();

    for path in files {
        let search_paths = vec![
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
            root.to_path_buf(),
        ];
        match compiler.compile_file_to_fir_with_lane(
            &path,
            &search_paths,
            SignalFirLane::TransformFastLane,
        ) {
            Ok(fir_out) => match compile_fir_to_cranelift_jit(
                &fir_out.store,
                fir_out.module,
                &CraneliftOptions::default(),
            ) {
                Ok(compiled) => {
                    if compiled.compute_body_lowered() {
                        lowered_ok.push(path);
                    } else {
                        let reason =
                            diagnose_cranelift_compute_subset_gap(&fir_out.store, fir_out.module)
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| "<subset-gap-unknown>".to_string());
                        *stub_reasons.entry(reason.clone()).or_insert(0) += 1;
                        if stub_examples.len() < 20 {
                            stub_examples.push((path.clone(), reason));
                        }
                        stub_ok.push(path);
                    }
                }
                Err(e) => errors.push((path, format!("backend: {e}"))),
            },
            Err(e) => errors.push((path, format!("pipeline: {e}"))),
        }
    }

    println!("Cranelift backend scan over tests/corpus/*.dsp (fast-lane FIR)");
    println!(
        "lowered_ok={} stub_ok={} errors={}",
        lowered_ok.len(),
        stub_ok.len(),
        errors.len()
    );

    if !stub_ok.is_empty() {
        println!("\nStub fallback examples (up to 20):");
        for (p, reason) in &stub_examples {
            println!("  {} => {}", p.display(), reason);
        }
    }

    if !stub_reasons.is_empty() {
        println!("\nStub fallback reasons (frequency):");
        let mut items: Vec<_> = stub_reasons.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (reason, count) in items.into_iter().take(20) {
            println!("  {:>3}  {}", count, reason);
        }
    }

    if !errors.is_empty() {
        println!("\nErrors (up to 25):");
        for (p, e) in errors.iter().take(25) {
            let first = e.lines().next().unwrap_or("");
            println!("  {} => {}", p.display(), first);
        }
        if files_were_filtered(&filters) {
            println!("\nFull error details (filtered run):");
            for (p, e) in &errors {
                println!("--- {} ---\n{}\n", p.display(), e);
            }
        }
    }

    if !lowered_ok.is_empty() {
        println!("\nLowered examples (up to 20):");
        for p in lowered_ok.iter().take(20) {
            println!("  {}", p.display());
        }
    }
}

fn files_were_filtered(filters: &[String]) -> bool {
    !filters.is_empty()
}
