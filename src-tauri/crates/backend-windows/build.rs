fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        panic!("backend-windows only compiles for Windows targets");
    }
}
