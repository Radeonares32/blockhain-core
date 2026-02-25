fn main() {
    println!("cargo:rerun-if-changed=proto/protocol.proto");

    prost_build::Config::new()
        .compile_protos(&["proto/protocol.proto"], &["proto/"])
        .expect("Failed to compile Protobuf schemas");
}
