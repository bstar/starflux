use anyhow::Result;
use std::{
    io::{self, Write},
    process::Command,
};
pub fn run(workspace: &str) -> Result<()> {
    let state = |s: &str| {
        let _ = Command::new("tmux")
            .args([
                "-L",
                &format!("starflux-{workspace}"),
                "set-option",
                "-p",
                "-t",
                "p1:0.0",
                "@starflux_state",
                s,
            ])
            .status();
    };
    println!("\x1b[1;32mS T A R / F L U X · demo agent\x1b[0m\r\n");
    println!(
        "This is a local fixture for testing the workspace.\r\nDrop a file here, then press Enter to inspect its bytes.\r\nType /work, /done or /ask to exercise agent status.\r\nYour shell is in the pane beside this one.\r\n"
    );
    loop {
        state("needs_input");
        print!("\x1b[34magent › \x1b[0m");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        match line {
            "/work" => {
                state("working");
                println!("Working for three seconds…");
                std::thread::sleep(std::time::Duration::from_secs(3));
                state("completed");
                println!("Task completed.")
            }
            "/done" => {
                state("completed");
                println!("Task completed.");
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            "/ask" => println!("Question: which task should I take next?"),
            "/quit" => break,
            _ => {
                let paths = shell_words::split(line).unwrap_or_else(|_| vec![line.into()]);
                for path in paths {
                    match std::fs::read(&path) {
                        Ok(bytes) => println!("Received {} bytes from {}", bytes.len(), path),
                        Err(_) => println!("Prompt: {path}"),
                    }
                }
            }
        }
    }
    Ok(())
}
