//! Build script: embeds the git revision for `--version`.
//! Build script: embeds the git revision for `--version`.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Debug clap command construction exceeds the Windows default 1 MiB stack.
        println!("cargo:rustc-link-arg-bin=gpui-agent=/STACK:8388608");
    }
}
