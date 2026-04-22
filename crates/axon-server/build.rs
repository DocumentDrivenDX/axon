use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rustc-check-cfg=cfg(axon_embed_ui_bundle)");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let ui_build_dir = manifest_dir.join("../../ui/build");
    println!("cargo:rerun-if-changed={}", ui_build_dir.display());
    if ui_build_dir.join("index.html").is_file() {
        println!("cargo:rustc-cfg=axon_embed_ui_bundle");
    }

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/axon.proto"], &["proto"])?;
    Ok(())
}
