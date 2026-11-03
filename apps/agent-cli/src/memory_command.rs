//! Client input and protocol values only; scope IDs remain daemon-owned.
use agent_protocol::v1::{MemoryKind, MemoryScopeKind, MemoryState};

pub const USAGE: &str = "/memory list [STATE|all] [AFTER_ID] · get ID · status · search TEXT · add SCOPE KIND TEXT · propose SCOPE KIND TEXT · approve ID REVISION · edit ID REVISION TEXT · forget ID REVISION · jobs ID · retry ID JOB_ID · rebuild · rebuilds [AFTER_ID]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryCommand {
  List {
    state: Option<MemoryState>,
    after: String,
  },
  Get(String),
  Status,
  Rebuild {
    request: String,
  },
  Rebuilds {
    after: String,
  },
  Search(String),
  Create {
    id: String,
    request: String,
    scope: MemoryScopeKind,
    kind: MemoryKind,
    state: MemoryState,
    content: String,
  },
  Approve {
    id: String,
    revision: u64,
    request: String,
  },
  Edit {
    id: String,
    revision: u64,
    request: String,
    content: String,
  },
  Forget {
    id: String,
    revision: u64,
  },
  Jobs(String),
  Retry {
    id: String,
    job: String,
  },
}

pub(crate) fn parse(args: &[String]) -> Result<MemoryCommand, String> {
  let command = match args {
    [] => MemoryCommand::List {
      state: None,
      after: String::new(),
    },
    [verb, rest @ ..] if verb == "list" && rest.len() <= 2 => MemoryCommand::List {
      state: rest
        .first()
        .map(|value| state(value))
        .transpose()?
        .flatten(),
      after: rest
        .get(1)
        .map(|id| identifier(id))
        .transpose()?
        .unwrap_or_default(),
    },
    [verb] if verb == "status" => MemoryCommand::Status,
    [verb] if verb == "rebuild" => MemoryCommand::Rebuild {
      request: uuid::Uuid::now_v7().to_string(),
    },
    [verb] if verb == "rebuilds" => MemoryCommand::Rebuilds {
      after: String::new(),
    },
    [verb, after] if verb == "rebuilds" => MemoryCommand::Rebuilds {
      after: identifier(after)?,
    },
    [verb, id] if verb == "get" => MemoryCommand::Get(identifier(id)?),
    [verb, id] if verb == "jobs" => MemoryCommand::Jobs(identifier(id)?),
    [verb, text @ ..] if verb == "search" => MemoryCommand::Search(content(text)?),
    [verb, scope, kind, text @ ..] if matches!(verb.as_str(), "add" | "propose") => {
      MemoryCommand::Create {
        id: uuid::Uuid::now_v7().to_string(),
        request: uuid::Uuid::now_v7().to_string(),
        scope: match scope.as_str() {
          "user" => MemoryScopeKind::User,
          "project" => MemoryScopeKind::Project,
          "session" => MemoryScopeKind::Session,
          _ => return Err("SCOPE must be user, project, or session".into()),
        },
        kind: match kind.as_str() {
          "preference" => MemoryKind::Preference,
          "fact" => MemoryKind::Fact,
          "procedure" => MemoryKind::Procedure,
          _ => return Err("KIND must be preference, fact, or procedure".into()),
        },
        state: if verb == "add" {
          MemoryState::Active
        } else {
          MemoryState::Proposed
        },
        content: content(text)?,
      }
    }
    [verb, id, revision] if verb == "approve" => MemoryCommand::Approve {
      id: identifier(id)?,
      revision: revision_number(revision)?,
      request: uuid::Uuid::now_v7().to_string(),
    },
    [verb, id, revision, text @ ..] if verb == "edit" => MemoryCommand::Edit {
      id: identifier(id)?,
      revision: revision_number(revision)?,
      request: uuid::Uuid::now_v7().to_string(),
      content: content(text)?,
    },
    [verb, id, revision] if verb == "forget" => MemoryCommand::Forget {
      id: identifier(id)?,
      revision: revision_number(revision)?,
    },
    [verb, id, job] if verb == "retry" => MemoryCommand::Retry {
      id: identifier(id)?,
      job: identifier(job)?,
    },
    _ => return Err(format!("Usage: {USAGE}")),
  };
  Ok(command)
}

fn state(value: &str) -> Result<Option<MemoryState>, String> {
  Ok(match value {
    "all" => None,
    "proposed" => Some(MemoryState::Proposed),
    "active" => Some(MemoryState::Active),
    "superseded" => Some(MemoryState::Superseded),
    "rejected" => Some(MemoryState::Rejected),
    "deleted" => Some(MemoryState::Deleted),
    _ => {
      return Err("STATE must be all, proposed, active, superseded, rejected, or deleted".into());
    }
  })
}

fn identifier(value: &str) -> Result<String, String> {
  uuid::Uuid::parse_str(value).map_err(|_| "ID must be a UUID".to_owned())?;
  Ok(value.into())
}

fn revision_number(value: &str) -> Result<u64, String> {
  value
    .parse::<u64>()
    .ok()
    .filter(|value| *value > 0 && *value < i64::MAX as u64)
    .ok_or_else(|| "REVISION must be a positive revision from /memory get".into())
}

fn content(words: &[String]) -> Result<String, String> {
  let text = words.join(" ");
  if text.trim().is_empty() || text.len() > 16_384 {
    return Err("TEXT must contain 1..=16384 UTF-8 bytes".into());
  }
  Ok(text)
}
