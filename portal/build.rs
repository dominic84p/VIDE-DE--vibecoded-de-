use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/pw.c");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let obj = format!("{out_dir}/pw.o");
    let lib = format!("{out_dir}/libvide_pw.a");

    let cflags = Command::new("pkg-config")
        .args(["--cflags", "libpipewire-0.3"])
        .output()
        .expect("pkg-config libpipewire-0.3");
    let flags: Vec<String> = String::from_utf8_lossy(&cflags.stdout)
        .split_whitespace()
        .map(String::from)
        .collect();

    let status = Command::new("gcc")
        .args(["-O2", "-fPIC", "-c", "src/pw.c", "-o", &obj])
        .args(&flags)
        .status()
        .expect("gcc pw.c");
    assert!(status.success(), "failed to compile src/pw.c");

    let status = Command::new("ar")
        .args(["rcs", &lib, &obj])
        .status()
        .expect("ar libvide_pw.a");
    assert!(status.success(), "failed to archive libvide_pw.a");

    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static=vide_pw");
    println!("cargo:rustc-link-lib=dylib=pipewire-0.3");
}
