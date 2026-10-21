use super::node::Node;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
  io::{Read, Write},
  path::PathBuf,
  sync::mpsc,
  time::Duration,
};

pub struct Terminal {
  master: Box<dyn MasterPty + Send>,
  child: Box<dyn Child + Send + Sync>,
  writer: Box<dyn Write + Send>,
  reader: mpsc::Receiver<Vec<u8>>,
  parser: vt100::Parser,
  transcript: Vec<u8>,
  evidence: PathBuf,
  #[cfg(unix)]
  original_termios: String,
}
impl Terminal {
  pub fn start(node: &Node) -> Self {
    let pair = native_pty_system()
      .openpty(PtySize {
        rows: 36,
        cols: 140,
        pixel_width: 0,
        pixel_height: 0,
      })
      .unwrap();
    #[cfg(unix)]
    let original_termios = format!("{:?}", pair.master.get_termios().expect("PTY termios"));
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_agent-cli"));
    command.args([
      "--endpoint",
      &node.endpoint(),
      "--data-dir",
      node.data.path().to_str().unwrap(),
    ]);
    command.env("LYONBROWN_TOKEN", node.daemon.auth_token());
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    // Acceptance verifies the color-capable terminal, independent of the
    // test runner's own plain-log preference. Product NO_COLOR remains intact.
    command.env_remove("NO_COLOR");
    command.env("RUST_LOG", "warn");
    command.cwd(node.workspace.path());
    for variable in [
      "HTTP_PROXY",
      "HTTPS_PROXY",
      "ALL_PROXY",
      "http_proxy",
      "https_proxy",
      "all_proxy",
    ] {
      command.env_remove(variable);
    }
    let child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let writer = pair.master.take_writer().unwrap();
    let mut input = pair.master.try_clone_reader().unwrap();
    let (sender, reader) = mpsc::channel();
    std::thread::spawn(move || {
      let mut buffer = [0_u8; 8192];
      while let Ok(count) = input.read(&mut buffer) {
        if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
          break;
        }
      }
    });
    let evidence = std::env::temp_dir()
      .join("lyonbrown-tui-acceptance")
      .join(uuid::Uuid::now_v7().to_string());
    std::fs::create_dir_all(&evidence).unwrap();
    Self {
      master: pair.master,
      child,
      writer,
      reader,
      parser: vt100::Parser::new(36, 140, 0),
      transcript: Vec::new(),
      evidence,
      #[cfg(unix)]
      original_termios,
    }
  }
  pub fn send(&mut self, input: &str) {
    self.writer.write_all(input.as_bytes()).unwrap();
    self.writer.flush().unwrap();
  }
  pub fn paste(&mut self, text: &str) {
    self.send(&format!("\x1b[200~{text}\x1b[201~"));
  }
  fn pump(&mut self) {
    while let Ok(bytes) = self.reader.try_recv() {
      self.parser.process(&bytes);
      self.transcript.extend(bytes);
    }
  }
  pub async fn screen(&mut self, text: &str) {
    self
      .wait_screen(text, |contents| contents.contains(text))
      .await;
  }
  pub async fn screen_without(&mut self, text: &str) {
    self
      .wait_screen(text, |contents| !contents.contains(text))
      .await;
  }
  async fn wait_screen(&mut self, text: &str, condition: impl Fn(&str) -> bool) {
    let result = tokio::time::timeout(Duration::from_secs(15), async {
      loop {
        self.pump();
        if condition(&self.parser.screen().contents()) {
          return;
        }
        assert!(
          self.child.try_wait().unwrap().is_none(),
          "CLI exited: {}",
          String::from_utf8_lossy(&self.transcript)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    })
    .await;
    self.snapshot("latest");
    assert!(
      result.is_ok(),
      "terminal never showed {text:?}: {}",
      self.parser.screen().contents()
    );
  }
  pub fn contents(&mut self) -> String {
    self.pump();
    self.parser.screen().contents()
  }
  pub fn snapshot(&mut self, name: &str) {
    self.pump();
    std::fs::write(
      self.evidence.join(format!("{name}.txt")),
      self.parser.screen().contents(),
    )
    .unwrap();
    std::fs::write(self.evidence.join("terminal.ansi"), &self.transcript).unwrap();
    let screen = self.parser.screen();
    let (rows, columns) = screen.size();
    let cells: Vec<_> = (0..rows)
      .flat_map(|row| {
        (0..columns).filter_map(move |column| {
          screen.cell(row, column).map(|cell| {
            serde_json::json!({
              "row": row, "column": column, "text": cell.contents(),
              "foreground": color(cell.fgcolor()), "background": color(cell.bgcolor()),
              "bold": cell.bold(), "italic": cell.italic(), "inverse": cell.inverse(),
              "wide": cell.is_wide(), "continuation": cell.is_wide_continuation(),
            })
          })
        })
      })
      .collect();
    let capture = serde_json::json!({
      "source": "actual agent-cli child process through portable-pty and vt100",
      "platform": std::env::consts::OS, "version": env!("CARGO_PKG_VERSION"),
      "rows": rows, "columns": columns, "cells": cells,
    });
    std::fs::write(
      self.evidence.join(format!("{name}.cells.json")),
      serde_json::to_vec(&capture).unwrap(),
    )
    .unwrap();
  }
  pub fn resize(&mut self, rows: u16, cols: u16) {
    self
      .master
      .resize(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
      })
      .unwrap();
    self.parser.screen_mut().set_size(rows, cols);
  }
  pub async fn quit(mut self) {
    self.send("\x03");
    let status = tokio::time::timeout(Duration::from_secs(5), async {
      loop {
        if let Some(status) = self.child.try_wait().unwrap() {
          return status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    })
    .await
    .expect("CLI must quit and reap its process");
    assert!(status.success());
    #[cfg(unix)]
    assert_eq!(
      format!(
        "{:?}",
        self.master.get_termios().expect("restored PTY termios")
      ),
      self.original_termios,
      "CLI must restore canonical terminal settings"
    );
    self.pump();
    assert!(
      self
        .transcript
        .windows(8)
        .any(|bytes| bytes == b"\x1b[?1049l"),
      "alternate screen must restore"
    );
    assert!(
      self
        .transcript
        .windows(8)
        .any(|bytes| bytes == b"\x1b[?2004l"),
      "paste mode must restore"
    );
    self.snapshot("quit");
    eprintln!("TUI PTY acceptance evidence: {}", self.evidence.display());
  }
}

fn color(color: vt100::Color) -> serde_json::Value {
  match color {
    vt100::Color::Default => serde_json::Value::Null,
    vt100::Color::Idx(index) => serde_json::json!({"index": index}),
    vt100::Color::Rgb(red, green, blue) => serde_json::json!([red, green, blue]),
  }
}
impl Drop for Terminal {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
    self.snapshot("final");
  }
}
