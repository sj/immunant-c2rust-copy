fn main() {
    let here = ::std::path::PathBuf::from(::std::env::("CARGO_MANIFEST_DIR")).unwrap());
    let cross_checks_path = here.parent()
        .and_then(|x| x.parent())
        .and_then(|x| x.parent())
        .unwrap();
    let libclevrbuf_path = cross_checks_path.join("ReMon").join("libclevrbuf");
    println!("cargo:rustc-link-lib=dylib=clevrbuf");
    println!("cargo:rustc-link-search=native={}", libclevrbuf_path.display());
}
