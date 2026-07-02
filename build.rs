fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(false)
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(
            &[
                "proto/coolercontrol/models/v1/device.proto",
                "proto/coolercontrol/device_service/v1/device_service.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
