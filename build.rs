// Puts the Ninho logo and the file details (product name, version, publisher) inside ninho.exe.
//   assets/ninho.res: the icon, made by build-icons.cjs from assets/ninho.svg.
//   OUT_DIR/version.res: the version details, written here from Cargo.toml so they always match the release
//   (Explorer shows them in Properties > Details; code signing requires product name and version to be set).
//   MSVC toolchain (GitHub Actions, x64 and x86): link.exe takes .res files as they are.
//   GNU toolchain: windres converts each to an object file first (no C preprocessor needed for a .res).
// Without either, the build still works, with the default icon and no details.
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/ninho.res");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let icon = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets").join("ninho.res");
    let version = out_dir.join("version.res");
    std::fs::write(&version, version_res(&std::env::var("CARGO_PKG_VERSION").unwrap())).expect("version.res");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for res in [&icon, &version] {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
        return;
    }
    let format = if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86") { "pe-i386" } else { "pe-x86-64" };
    for (res, name) in [(&icon, "ninho-res.o"), (&version, "version-res.o")] {
        let obj = out_dir.join(name);
        if windres(res, &obj, format) {
            println!("cargo:rustc-link-arg-bins={}", obj.display());
        } else {
            println!("cargo:warning=windres not found: ninho.exe keeps the default icon and has no version details");
            return;
        }
    }
}

fn windres(res: &Path, obj: &Path, format: &str) -> bool {
    std::process::Command::new("windres")
        .args(["-J", "res", "-O", "coff", "-F", format, "-i"])
        .arg(res)
        .arg("-o")
        .arg(obj)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---- VS_VERSIONINFO as a .res file (the binary format rc.exe would write) ----

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect()
}

fn pad4(v: &mut Vec<u8>) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
}

/// One version block: wLength, wValueLength, wType, key, value, children (each starting on a 4-byte boundary).
fn block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    b.extend(utf16z(key));
    pad4(&mut b);
    b.extend(value);
    for c in children {
        pad4(&mut b);
        b.extend(c);
    }
    let len = b.len() as u16;
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b[2..4].copy_from_slice(&value_len.to_le_bytes());
    b[4..6].copy_from_slice(&(text as u16).to_le_bytes());
    b
}

fn string(key: &str, value: &str) -> Vec<u8> {
    let v = utf16z(value);
    block(key, &v, (v.len() / 2) as u16, true, &[])
}

fn version_res(version: &str) -> Vec<u8> {
    let n: Vec<u32> = version.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).chain([0, 0, 0]).take(3).collect();
    let (ms, ls) = ((n[0] << 16) | n[1], n[2] << 16);
    let mut fixed = Vec::new();
    for v in [0xFEEF_04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend(v.to_le_bytes());
    }
    let strings = [
        string("CompanyName", "phmrmkv"),
        string("FileDescription", "Ninho"),
        string("FileVersion", version),
        string("InternalName", "ninho"),
        string("LegalCopyright", "© 2026 phmrmkv. MIT or GPL-3.0."),
        string("OriginalFilename", "ninho.exe"),
        string("ProductName", "Ninho"),
        string("ProductVersion", version),
    ];
    let table = block("040904B0", &[], 0, true, &strings);
    let sfi = block("StringFileInfo", &[], 0, true, &[table]);
    let translation: Vec<u8> = [0x0409u16, 0x04B0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let var = block("Translation", &translation, 4, false, &[]);
    let vfi = block("VarFileInfo", &[], 0, true, &[var]);
    let data = block("VS_VERSION_INFO", &fixed, fixed.len() as u16, false, &[sfi, vfi]);

    // A .res file starts with an empty entry, then: DataSize, HeaderSize, TYPE=RT_VERSION(16), NAME=1,
    // DataVersion, MemoryFlags, LanguageId (en-US), Version, Characteristics.
    let mut res = vec![0u8, 0, 0, 0, 0x20, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0];
    res.extend([0u8; 16]);
    res.extend((data.len() as u32).to_le_bytes());
    res.extend(32u32.to_le_bytes());
    res.extend([0xFF, 0xFF, 16, 0, 0xFF, 0xFF, 1, 0]);
    res.extend(0u32.to_le_bytes());
    res.extend(0x0030u16.to_le_bytes());
    res.extend(0x0409u16.to_le_bytes());
    res.extend(0u32.to_le_bytes());
    res.extend(0u32.to_le_bytes());
    res.extend(&data);
    pad4(&mut res);
    res
}
