//! Generates Rust IPC contract types from the checked-in Protobuf schema.

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let mut config = prost_build::Config::new();
    config
        .boxed(".screensearch.v1.SearchEvent.event.citation")
        .protoc_executable(protoc)
        .compile_protos(&["proto/screensearch/v1/screensearch.proto"], &["proto"])
        .expect("IPC Protobuf contracts compile");

    println!("cargo:rerun-if-changed=proto/screensearch/v1/screensearch.proto");
}
