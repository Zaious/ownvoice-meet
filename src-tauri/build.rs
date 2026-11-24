use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        link_compiler_rt();
    }
    tauri_build::build()
}

/// llama.cpp's Metal code checks the macOS version with `@available`, which clang turns into calls
/// to `__isPlatformVersionAtLeast` when the app targets an older macOS (ours: 11). That function
/// lives in clang's compiler-rt (`libclang_rt.osx.a`), which rustc does not link, so the release
/// build failed with an undefined symbol. Link it from the clang that Xcode provides.
fn link_compiler_rt() {
    let out = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output();
    let dir = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            println!("cargo:warning=could not ask clang for its resource dir; not linking compiler-rt");
            return;
        }
    };
    let lib_dir = std::path::Path::new(&dir).join("lib").join("darwin");
    if !lib_dir.join("libclang_rt.osx.a").is_file() {
        println!(
            "cargo:warning=no libclang_rt.osx.a in {}; not linking compiler-rt",
            lib_dir.display()
        );
        return;
    }
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
