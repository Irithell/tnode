fn main() {
    println!("cargo:rustc-link-arg=-Wl,--version-script=crates/thook/export.map");
}
