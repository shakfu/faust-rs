//! Verifies the local libfaust C/C++ distribution surface.
//!
//! The check intentionally exercises the installed shape of the Rust port:
//! build the unified `faust-ffi` dynamic library, compare exported symbols
//! against maintained C headers, and syntax-check tiny C and C++ clients.

use super::*;

/// Builds `faust-ffi`, checks exported C symbols against local headers, and
/// syntax-checks tiny C/C++ clients using the maintained wrapper headers.
pub(crate) fn libfaust_export_check() -> Result<(), Box<dyn std::error::Error>> {
    build_faust_ffi()?;

    let workspace = workspace_root();
    let dynamic_library = workspace.join("target").join("debug").join(format!(
        "{}faust{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    if !dynamic_library.exists() {
        return Err(format!(
            "expected faust dynamic library after build: {}",
            dynamic_library.display()
        )
        .into());
    }

    let expected = expected_header_symbols(&workspace)?;
    let exported = exported_dynamic_symbols(&dynamic_library)?;
    let missing = expected
        .difference(&exported)
        .cloned()
        .collect::<Vec<String>>();
    if !missing.is_empty() {
        return Err(format!(
            "faust dynamic library is missing header-declared exports: {}",
            missing.join(", ")
        )
        .into());
    }

    syntax_check_headers(&workspace)?;

    println!(
        "libfaust-export-check: {} header symbols exported by {}",
        expected.len(),
        workspace_relative_path(&dynamic_library)
    );
    Ok(())
}

fn build_faust_ffi() -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("cargo")
        .args(["build", "-p", "faust-ffi"])
        .status()?;
    if !status.success() {
        return Err("cargo build -p faust-ffi failed".into());
    }
    Ok(())
}

fn expected_header_symbols(
    workspace: &Path,
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let headers = [
        workspace.join("crates/box-ffi/include/libfaust-box-c.h"),
        workspace.join("crates/signal-ffi/include/libfaust-signal-c.h"),
    ];
    let mut symbols = BTreeSet::new();
    for header in headers {
        for symbol in parse_c_header_function_symbols(&fs::read_to_string(&header)?) {
            symbols.insert(symbol);
        }
    }
    Ok(symbols)
}

fn parse_c_header_function_symbols(header: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut pending = String::new();

    for raw_line in header.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("//")
            || line == "{"
            || line == "}"
            || line == "};"
            || line == "extern \"C\" {"
        {
            continue;
        }

        if pending.is_empty() && (line.starts_with("typedef ") || line.starts_with("enum ")) {
            continue;
        }

        if pending.is_empty() && !line.contains('(') {
            continue;
        }

        if !pending.is_empty() {
            pending.push(' ');
        }
        pending.push_str(line);

        if pending.ends_with(';') {
            if let Some(name) = extract_c_function_name(&pending) {
                symbols.push(name);
            }
            pending.clear();
        }
    }

    symbols.sort();
    symbols.dedup();
    symbols
}

fn exported_dynamic_symbols(path: &Path) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let output = if cfg!(target_os = "macos") {
        Command::new("nm").args(["-gU"]).arg(path).output()?
    } else if cfg!(target_os = "windows") {
        Command::new("dumpbin").arg("/exports").arg(path).output()?
    } else {
        Command::new("nm")
            .args(["-D", "--defined-only"])
            .arg(path)
            .output()?
    };

    if !output.status.success() {
        return Err(format!(
            "failed to inspect dynamic symbols for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_exported_symbol_lines(&stdout))
}

fn parse_exported_symbol_lines(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .last()
                .map(|name| name.trim_start_matches('_').to_string())
                .filter(|name| is_libfaust_c_symbol(name))
        })
        .collect()
}

fn is_libfaust_c_symbol(name: &str) -> bool {
    name.starts_with('C')
        || matches!(
            name,
            "createLibContext" | "destroyLibContext" | "freeCMemory"
        )
}

fn syntax_check_headers(workspace: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = workspace.join("target/libfaust-export-check");
    fs::create_dir_all(&out_dir)?;

    let c_file = out_dir.join("smoke.c");
    fs::write(
        &c_file,
        r#"#include "libfaust-box-c.h"
#include "libfaust-signal-c.h"

int main(void) {
    Signal s = CsigInput(0);
    Box b = CboxWire();
    return (s == 0 || b == 0) ? 0 : 0;
}
"#,
    )?;

    let cpp_file = out_dir.join("smoke.cpp");
    fs::write(
        &cpp_file,
        r#"#include "libfaust-box.h"
#include "libfaust-signal.h"

int main() {
    Signal x = sigInput(0);
    Signal y = sigMul(x, sigReal(0.5));
    int op = 0;
    Signal a = nullptr;
    Signal b = nullptr;
    return isSigBinOp(y, op, a, b) ? 0 : 0;
}
"#,
    )?;

    syntax_check_c_like(&c_file, "c")?;
    syntax_check_c_like(&cpp_file, "c++")?;
    Ok(())
}

fn syntax_check_c_like(path: &Path, language: &str) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = workspace_root();
    let include_dirs = [
        workspace.join("crates/box-ffi/include"),
        workspace.join("crates/signal-ffi/include"),
    ];

    let compiler_var = if language == "c" { "CC" } else { "CXX" };
    let default_compiler = if cfg!(target_os = "windows") {
        "cl"
    } else if language == "c" {
        "cc"
    } else {
        "c++"
    };
    let compiler = std::env::var(compiler_var).unwrap_or_else(|_| default_compiler.to_string());

    let mut command = Command::new(&compiler);
    if cfg!(target_os = "windows") && compiler.ends_with("cl") {
        command.arg("/nologo").arg("/Zs");
        command.arg(if language == "c" {
            "/std:c11"
        } else {
            "/std:c++17"
        });
        for include_dir in include_dirs {
            command.arg(format!("/I{}", include_dir.display()));
        }
        command.arg(path);
    } else {
        command.arg(if language == "c" {
            "-std=c11"
        } else {
            "-std=c++17"
        });
        command.arg("-fsyntax-only");
        for include_dir in include_dirs {
            command.arg("-I").arg(include_dir);
        }
        command.arg(path);
    }

    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "{} syntax check failed for {}:\n{}{}",
            compiler,
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_header_function_symbols_without_libfaust_macro() {
        let header = r#"
            #ifdef __cplusplus
            extern "C" {
            #endif
            typedef CTree* Signal;
            enum SType { kSInt, kSReal };
            void createLibContext(void);
            Signal CsigFFun(enum SType rtype, const char** names,
                            enum SType* atypes, const char* incfile);
            #ifdef __cplusplus
            }
            #endif
        "#;

        assert_eq!(
            parse_c_header_function_symbols(header),
            vec!["CsigFFun".to_string(), "createLibContext".to_string()]
        );
    }

    #[test]
    fn parses_nm_and_dumpbin_symbol_lines() {
        let output = r#"
            0000000000012340 T _CsigInt
            0000000000012350 T _createLibContext
              12    B 0000000180001230 CboxInt
            0000000000012360 T _rust_internal_helper
        "#;

        let symbols = parse_exported_symbol_lines(output);

        assert!(symbols.contains("CsigInt"));
        assert!(symbols.contains("CboxInt"));
        assert!(symbols.contains("createLibContext"));
        assert!(!symbols.contains("rust_internal_helper"));
    }
}
