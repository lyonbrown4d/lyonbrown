use std::{
  io::{Read, Write},
  process::{Command, Stdio},
  thread,
  time::Duration,
};

#[allow(
  clippy::zombie_processes,
  reason = "tree-exit intentionally leaves a live descendant for supervisor cleanup verification"
)]
fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  match args.first().map(String::as_str) {
    Some("report") => {
      for argument in args.iter().skip(1) {
        println!("arg:{argument}");
      }
      println!("cwd:{}", std::env::current_dir().unwrap().display());
      println!(
        "selected:{}",
        std::env::var("LYONBROWN_FIXTURE_SELECTED").unwrap_or_default()
      );
      println!("ambient:{}", std::env::var("HOME").unwrap_or_default());
      println!("extra:{}", std::env::var("PATH").unwrap_or_default());
      let mut stdin = Vec::new();
      std::io::stdin().read_to_end(&mut stdin).unwrap();
      println!("stdin:{}", stdin.len());
      eprint!("stderr:确认");
    }
    Some("mark") => {
      std::fs::write(&args[1], "executed").unwrap();
    }
    Some("dual") => {
      std::io::stdout().write_all(&vec![b'o'; 100_000]).unwrap();
      std::io::stderr().write_all(&vec![b'e'; 100_000]).unwrap();
    }
    Some("flood") => {
      let block = [b'x'; 4096];
      loop {
        if std::io::stdout().write_all(&block).is_err() {
          break;
        }
        if std::io::stderr().write_all(&block).is_err() {
          break;
        }
      }
      std::fs::write(&args[1], "must not finish flood").unwrap();
    }
    Some("descendant") => {
      std::fs::write(&args[1], "ready").unwrap();
      thread::sleep(Duration::from_millis(1200));
      std::fs::write(&args[2], "escaped cleanup").unwrap();
    }
    Some("tree") | Some("tree-exit") => {
      let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["descendant", &args[1], &args[2]])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
      if args[0] == "tree" {
        // Keep the handle until the child exits normally; the supervisor must terminate both.
        let _ = child.wait();
      } else {
        // Prove the descendant reached its body before the leader exits.
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while !std::path::Path::new(&args[1]).exists() && std::time::Instant::now() < until {
          thread::sleep(Duration::from_millis(5));
        }
      }
    }
    Some("fail") => {
      eprint!("known failure");
      std::process::exit(17);
    }
    _ => std::process::exit(2),
  }
}
