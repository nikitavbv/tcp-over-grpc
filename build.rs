use std::{io::Result, env, path::PathBuf};

fn main() -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("tcp_over_grpc_descriptor.bin"))
        .compile(&["proto/tcp-over-grpc.proto"], &["proto"])?;
    Ok(())
}
