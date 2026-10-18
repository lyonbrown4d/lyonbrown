use agent_browser::{Command, Reply};
pub(super) fn matches_reply(command: &Command, reply: &Reply, session: &str) -> bool {
  match (command, reply) {
    (
      Command::OpenSpace {
        session_id,
        profile_id,
        headless,
      },
      Reply::Space(space),
    ) => {
      !space.id.is_empty()
        && &space.session_id == session_id
        && &space.profile_id == profile_id
        && space.headless == *headless
    }
    (
      Command::Takeover {
        space_id,
        human_control,
      },
      Reply::Space(space),
    ) => {
      &space.id == space_id && space.session_id == session && space.human_control == *human_control
    }
    (Command::OpenTab { space_id, .. }, Reply::Tab(tab)) => {
      !tab.id.is_empty() && &tab.space_id == space_id
    }
    (Command::Act(action), Reply::Snapshot(snapshot)) => {
      snapshot.tab_id == action.tab_id
        && !snapshot.id.is_empty()
        && snapshot.accessibility_json.len() <= agent_browser::MAX_AX_BYTES
        && snapshot.frame.as_ref().is_none_or(|frame| {
          frame.tab_id == snapshot.tab_id
            && frame.revision == snapshot.revision
            && frame.validate().is_ok()
        })
    }
    (Command::CloseSpace { .. } | Command::Input { .. }, Reply::Done) => true,
    _ => false,
  }
}
