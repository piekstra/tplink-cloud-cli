fn main() {
    println!("cargo:rerun-if-changed=certs/tplink-ca-chain.pem");
}
