// Puts the Ninho logo inside ninho.exe (the icon Explorer and the taskbar show). assets/ninho.res is made by
// build-icons.cjs from assets/ninho.svg.
//   MSVC toolchain (GitHub Actions, x64 and x86): link.exe takes the .res file as it is.
//   GNU toolchain: windres converts it to an object file first (no C preprocessor needed for a .res).
// Without either, the build still works, with the default icon.
fn main() {
    println!("cargo:rerun-if-changed=assets/ninho.res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let res = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets").join("ninho.res");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins={}", res.display());
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("ninho-res.o");
    let format = if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86") { "pe-i386" } else { "pe-x86-64" };
    let ok = std::process::Command::new("windres")
        .args(["-J", "res", "-O", "coff", "-F", format, "-i"])
        .arg(&res)
        .arg("-o")
        .arg(&out)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", out.display());
    } else {
        println!("cargo:warning=windres not found: ninho.exe keeps the default icon");
    }
}
