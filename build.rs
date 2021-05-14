fn main() {
    cc::Build::new()
        .file("src/jpeg_compressor.c")
        .compile("jpeg_compressor");
    println!("cargo:rustc-link-lib=static=jpeg");
}
