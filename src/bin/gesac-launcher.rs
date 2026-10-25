fn main() {
    match gesac::update::launcher_main() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("GESAC launcher: {error}");
            std::process::exit(1);
        }
    }
}
