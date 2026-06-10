fn main() {
    let args = std::env::args().skip(1).collect();
    std::process::exit(afs_cli::run(args));
}
