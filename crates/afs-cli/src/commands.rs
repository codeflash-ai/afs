const COMMANDS: &[&str] = &[
    "connect", "mount", "status", "pull", "push", "diff", "undo", "log", "resolve", "config",
];

pub fn dispatch(args: &[String]) -> i32 {
    if args.is_empty() || has_flag(args, "--help") || has_flag(args, "-h") {
        print_help();
        return 0;
    }

    let json = has_flag(args, "--json");
    match args[0].as_str() {
        "connect" => stub("connect", json),
        "mount" => stub("mount", json),
        "status" => stub("status", json),
        "pull" => stub("pull", json),
        "push" => stub("push", json),
        "diff" => stub("diff", json),
        "undo" => stub("undo", json),
        "log" => stub("log", json),
        "resolve" => stub("resolve", json),
        "config" => stub("config", json),
        command => {
            eprintln!("unknown command: {command}");
            print_help();
            2
        }
    }
}

fn stub(command: &str, json: bool) -> i32 {
    if json {
        println!("{{\"ok\":false,\"command\":\"{command}\",\"error\":\"not_implemented\"}}");
    } else {
        println!("afs {command}: not implemented yet");
    }

    0
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn print_help() {
    println!("afs <command> [options]");
    println!();
    println!("Commands:");
    for command in COMMANDS {
        println!("  {command}");
    }
}
