fn main() {
    // Tell cargo to recompile if this python file changes
    println!("cargo:rerun-if-changed=python_assets/tools.py");
}
