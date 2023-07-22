use std::env;
use std::path::PathBuf;

fn main() {
  let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
  println!("cargo:rustc-link-search=native={}/build", dir);
  println!("cargo:rustc-link-lib=dylib=mosaik-decoder");
  println!("cargo:rerun-if-changed=src");

  let bindings = bindgen::Builder::default()
    .header("src/api.hpp")
    .allowlist_file("src/Decoder.h")
    .allowlist_type("Decoder")
    .allowlist_recursively(false)
    .blocklist_item("std::unique_ptr")
    .merge_extern_blocks(true)
    .parse_callbacks(Box::new(bindgen::CargoCallbacks))
    .generate()
    .expect("Unable to generate bindings");

  let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
  bindings
    .write_to_file(out_path.join("bindings.rs"))
    .expect("Couldn't write bindings!");
}
