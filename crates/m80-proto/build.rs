//! Build m80 protobuf wire types from the checked-in schema.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);

    prost_build::Config::new().compile_protos(&["proto/m80/wire.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/m80/wire.proto");
    Ok(())
}
