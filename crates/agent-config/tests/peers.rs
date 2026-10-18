use std::collections::BTreeMap;

use agent_config::load_with_environment;

#[test]
fn peer_listener_is_opt_in_and_uses_the_same_environment_layers() {
  let data = tempfile::tempdir().unwrap();
  let defaults = load_with_environment(data.path(), None, &BTreeMap::new()).unwrap();
  assert!(defaults.peer.is_none());
  let environment = BTreeMap::from([
    ("LYONBROWN_PEER_LISTEN".into(), "0.0.0.0:7448".into()),
    (
      "LYONBROWN_PEER_ADVERTISE_URL".into(),
      "https://192.0.2.1:7448".into(),
    ),
    ("LYONBROWN_PEER_MDNS".into(), "true".into()),
  ]);
  let settings = load_with_environment(data.path(), None, &environment).unwrap();
  let peer = settings.peer.unwrap();
  assert_eq!(peer.listen.to_string(), "0.0.0.0:7448");
  assert_eq!(peer.advertise_url, "https://192.0.2.1:7448");
  assert!(peer.mdns);
  assert!(settings.listen.ip().is_loopback());
}

#[test]
fn peer_advertisement_requires_complete_https_origin_and_listen_port() {
  for invalid in [
    "http://192.0.2.1:7448",
    "https://0.0.0.0:7448",
    "https://*:7448",
    "https://[::]:7448",
    "https://peer.example:443",
    "https://peer.example:7448/path",
    "https://user:secret@peer.example:7448",
    "https://peer.example:7448?secret=x",
  ] {
    let settings = agent_config::PeerSettings {
      listen: "0.0.0.0:7448".parse().unwrap(),
      advertise_url: invalid.into(),
      mdns: false,
    };
    assert!(settings.validate().is_err(), "{invalid}");
  }
}
