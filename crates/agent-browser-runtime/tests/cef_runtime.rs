use agent_browser::{
  Action, BrowserError, BrowserHost, Command, Reply, Snapshot, Space, Tab, ViewInput,
};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn snapshot(host: &BrowserHost, tab: &Tab, needle: &str) -> Snapshot {
  let mut last = String::new();
  let mut stale = 0;
  let result = tokio::time::timeout(Duration::from_secs(20), async {
    loop {
      match host
        .request(Command::Observe {
          tab_id: tab.id.clone(),
          screenshot: true,
        })
        .await
      {
        Ok(Reply::Snapshot(snapshot))
          if snapshot.accessibility_json.contains(needle) && snapshot.frame.is_some() =>
        {
          return snapshot;
        }
        Ok(Reply::Snapshot(snapshot)) => {
          last = format!(
            "frame={:?}, AX={}",
            snapshot
              .frame
              .as_ref()
              .map(|frame| (frame.width, frame.height, frame.sequence)),
            snapshot.accessibility_json
          );
        }
        Err(BrowserError::StaleSnapshot) => {
          stale += 1;
        }
        reply => panic!("native Chromium AX observation failed: {reply:?}"),
      }
      tokio::time::sleep(Duration::from_millis(60)).await;
    }
  })
  .await;
  result.unwrap_or_else(|_| panic!("native page ready timed out ({stale} stale reads): {last}"))
}
async fn space(host: &BrowserHost, profile: &str, headless: bool) -> Space {
  match host
    .request(Command::OpenSpace {
      session_id: "native-acceptance".into(),
      profile_id: profile.into(),
      headless,
    })
    .await
    .expect("isolated native profile")
  {
    Reply::Space(space) => space,
    _ => panic!("expected space"),
  }
}
async fn tab(host: &BrowserHost, space: &Space, url: &str) -> Tab {
  match host
    .request(Command::OpenTab {
      space_id: space.id.clone(),
      url: url.into(),
    })
    .await
    .expect("real Chromium navigation")
  {
    Reply::Tab(tab) => tab,
    _ => panic!("expected tab"),
  }
}
fn element(snapshot: &Snapshot, name: &str) -> String {
  serde_json::from_str::<Vec<Value>>(&snapshot.accessibility_json)
    .expect("semantic AX JSON")
    .into_iter()
    .find(|node| {
      node["name"] == name && matches!(node["role"].as_str(), Some("button" | "textbox"))
    })
    .expect("named AX element")["ref"]
    .as_str()
    .expect("semantic reference")
    .into()
}

/// Use a bundled native-cef client with authorized macOS Keychain access, or the
/// debug-only acceptance example with synthetic profiles and Chromium test keychain.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a real built native-cef client and installed CEF runtime"]
async fn chromium_profiles_frames_semantic_actions_staleness_and_takeover() {
  let client = PathBuf::from(
    std::env::var_os("LYONBROWN_NATIVE_CLIENT").expect("explicit native-cef acceptance executable"),
  );
  let profiles = tempfile::tempdir()
    .expect("private profile directory")
    .keep()
    .join("desktop");
  eprintln!(
    "Preserved native CEF acceptance profiles: {}",
    profiles.display()
  );
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("local fixture site");
  let url = format!("http://{}/", listener.local_addr().expect("site address"));
  let server = tokio::spawn(async move {
    loop {
      let Ok((mut socket, _)) = listener.accept().await else {
        break;
      };
      tokio::spawn(async move {
        let mut request = [0; 4096];
        if socket.read(&mut request).await.unwrap_or(0) == 0 {
          return;
        }
        let html = r#"<!doctype html><meta charset="utf-8"><title>Native acceptance</title><label>Name<input aria-label="Name" style="position:fixed;left:10%;top:40%;width:80%;height:20%;box-sizing:border-box"></label><input aria-label="Password" type="password" value="synthetic-browser-password"><button onclick="document.getElementById('state').textContent='Changed';document.cookie='native_profile=retained;max-age=3600'">Change text</button><p id="state">Ready</p><p id="cookie"></p><script>document.getElementById('cookie').textContent=document.cookie||'Fresh profile'</script>"#;
        let response = format!(
          "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
          html.len()
        );
        socket
          .write_all(response.as_bytes())
          .await
          .expect("send fixture response");
      });
    }
  });
  let host = BrowserHost::spawn(&client, profiles.as_path())
    .await
    .expect("actual CEF subprocess initialization");
  let collision = tokio::time::timeout(
    Duration::from_secs(10),
    BrowserHost::spawn(&client, profiles.as_path()),
  )
  .await
  .expect("same-product root collision must fail before CEF startup");
  assert!(matches!(collision, Err(BrowserError::Unavailable(_))));
  assert!(
    std::fs::read_to_string(profiles.join("runtime-logs/native.stderr.log"))
      .expect("runtime collision diagnostic")
      .contains("browser runtime cache root is unavailable or already owned"),
    "report runtime ownership before Chromium's singleton startup handling"
  );
  eprintln!("Concurrent same-product runtime root rejected without interrupting the owner");
  let first = space(&host, "first", false).await;
  let page = tab(&host, &first, &url).await;
  let observed = snapshot(&host, &page, "Change text").await;
  eprintln!("Native initial AX and frame received");
  assert!(
    !observed
      .accessibility_json
      .contains("synthetic-browser-password"),
    "native browser semantics must not disclose password values"
  );
  observed
    .frame
    .as_ref()
    .expect("actual native OSR pixels")
    .validate()
    .expect("owned bounded BGRA frame");
  let click = Action {
    id: uuid::Uuid::now_v7().to_string(),
    tab_id: page.id.clone(),
    snapshot_id: observed.id.clone(),
    revision: observed.revision,
    kind: "click".into(),
    element_ref: element(&observed, "Change text"),
    text: String::new(),
    timeout_ms: 5000,
  };
  host
    .request(Command::Takeover {
      space_id: first.id.clone(),
      human_control: true,
    })
    .await
    .expect("explicit human takeover");
  assert!(matches!(
    host.request(Command::Act(click)).await,
    Err(BrowserError::HumanControl)
  ));
  host
    .request(Command::Takeover {
      space_id: first.id.clone(),
      human_control: false,
    })
    .await
    .expect("return control");
  let observed = snapshot(&host, &page, "Change text").await;
  let fill = Action {
    id: uuid::Uuid::now_v7().to_string(),
    tab_id: page.id.clone(),
    snapshot_id: observed.id.clone(),
    revision: observed.revision,
    kind: "fill".into(),
    element_ref: element(&observed, "Name"),
    text: "Native input".into(),
    timeout_ms: 5000,
  };
  host
    .request(Command::Act(fill))
    .await
    .expect("real semantic field fill");
  let observed = snapshot(&host, &page, "Native input").await;
  eprintln!("Native semantic fill verified");
  let click = Action {
    id: uuid::Uuid::now_v7().to_string(),
    tab_id: page.id.clone(),
    snapshot_id: observed.id.clone(),
    revision: observed.revision,
    kind: "click".into(),
    element_ref: element(&observed, "Change text"),
    text: String::new(),
    timeout_ms: 5000,
  };
  host
    .request(Command::Act(click.clone()))
    .await
    .expect("real semantic click");
  snapshot(&host, &page, "Changed").await;
  eprintln!("Native semantic click and repaint verified");
  assert!(matches!(
    host.request(Command::Act(click)).await,
    Err(BrowserError::StaleSnapshot)
  ));
  host
    .request(Command::Takeover {
      space_id: first.id.clone(),
      human_control: true,
    })
    .await
    .expect("human input takeover");
  let current = snapshot(&host, &page, "Changed").await;
  let frame = current.frame.as_ref().expect("human frame");
  let (x, y) = (frame.width as i32 / 2, frame.height as i32 / 2);
  for event in [
    ViewInput::MouseButton {
      x,
      y,
      button: 0,
      pressed: true,
    },
    ViewInput::MouseButton {
      x,
      y,
      button: 0,
      pressed: false,
    },
    ViewInput::Text(" direct native input".into()),
  ] {
    host
      .request(Command::Input {
        tab_id: page.id.clone(),
        revision: current.revision,
        event,
      })
      .await
      .expect("actual pointer/text input");
  }
  snapshot(&host, &page, "direct native input").await;
  eprintln!("Native pointer focus and committed text verified");
  host
    .request(Command::Takeover {
      space_id: first.id.clone(),
      human_control: false,
    })
    .await
    .expect("return native input control");
  let second = space(&host, "second", true).await;
  assert!(
    second.headless,
    "headless space uses the same real Chromium API"
  );
  let isolated = tab(&host, &second, &url).await;
  let clean = snapshot(&host, &isolated, "Fresh profile").await;
  eprintln!("Native profile isolation verified");
  assert!(!clean.accessibility_json.contains("native_profile=retained"));
  host
    .request(Command::CloseSpace { space_id: first.id })
    .await
    .expect("close real profile and flush cookies");
  host
    .request(Command::Shutdown)
    .await
    .expect("first runtime fully exits and flushes persistent profiles");
  let host = BrowserHost::spawn(&client, profiles.as_path())
    .await
    .expect("fresh CEF process opens persistent profile");
  let restored = space(&host, "first", false).await;
  let restored_page = tab(&host, &restored, &url).await;
  snapshot(&host, &restored_page, "native_profile=retained").await;
  eprintln!("Native profile persistence across runtime restart verified");
  host
    .request(Command::Shutdown)
    .await
    .expect("CEF browsers close before shutdown");
  server.abort();
}
