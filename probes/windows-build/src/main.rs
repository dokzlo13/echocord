fn main() {
    println!(
        "Echocord build probe: os={}, arch={}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}
