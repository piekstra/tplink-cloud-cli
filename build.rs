fn main() {
    println!("cargo:rerun-if-changed=certs/tplink-ca-chain.pem");
    // Bake in the target triple for self-update asset selection.
    println!(
        "cargo:rustc-env=BUILD_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
