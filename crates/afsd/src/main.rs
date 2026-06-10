fn main() {
    let config = afsd::DaemonConfig::default();
    let daemon = afsd::Daemon::new(config);

    if let Err(error) = daemon.run_foreground() {
        eprintln!("afsd failed: {error}");
        std::process::exit(1);
    }
}
