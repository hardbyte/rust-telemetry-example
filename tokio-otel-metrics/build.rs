fn main() {
    println!("cargo:rustc-check-cfg=cfg(tokio_unstable)");
    if std::env::var("CARGO_CFG_TOKIO_UNSTABLE").is_ok() {
        println!("cargo:rustc-cfg=tokio_unstable");
    }
}
