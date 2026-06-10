pub mod commands;

pub fn run(args: Vec<String>) -> i32 {
    commands::dispatch(&args)
}
