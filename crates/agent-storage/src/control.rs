use anyhow::{Result, ensure};
use diesel::prelude::*;

use crate::Store;

diesel::table! {
  control_directory (node_id) {
    node_id -> Text,
    record -> Text,
    expires_at_ms -> BigInt,
  }
}
diesel::table! {
  control_links (source, target) {
    source -> Text,
    target -> Text,
  }
}

impl Store {
  pub fn control_replace_links(&self, links: &[(String, String)]) -> Result<()> {
    use self::control_links::dsl::*;
    ensure!(
      links
        .iter()
        .all(|(from, to)| !from.is_empty() && !to.is_empty() && from != to),
      "invalid control link"
    );
    self.lock()?.immediate_transaction(|connection| {
      diesel::delete(control_links).execute(connection)?;
      let rows = links
        .iter()
        .map(|(from, to)| (source.eq(from), target.eq(to)))
        .collect::<Vec<_>>();
      diesel::insert_or_ignore_into(control_links)
        .values(&rows)
        .execute(connection)?;
      Ok(())
    })
  }
  /// Only an authenticated control service may register a node's public metadata.
  pub fn control_register(&self, id: &str, encoded_record: &str, expires: i64) -> Result<()> {
    use self::control_directory::dsl::*;
    diesel::insert_into(control_directory)
      .values((
        node_id.eq(id),
        self::control_directory::record.eq(encoded_record),
        expires_at_ms.eq(expires),
      ))
      .on_conflict(node_id)
      .do_update()
      .set((
        self::control_directory::record.eq(encoded_record),
        expires_at_ms.eq(expires),
      ))
      .execute(&mut *self.lock()?)?;
    Ok(())
  }

  /// Expiration is checked on every lookup, including after process restart.
  pub fn control_resolve(&self, id: &str, now: i64) -> Result<Option<String>> {
    use self::control_directory::dsl::*;
    Ok(
      control_directory
        .find(id)
        .filter(expires_at_ms.gt(now))
        .select(record)
        .first(&mut *self.lock()?)
        .optional()?,
    )
  }

  /// Local operator authority; network RPCs must never call this method.
  pub fn control_allow_link(&self, from: &str, to: &str, allowed: bool) -> Result<()> {
    use self::control_links::dsl::*;
    ensure!(
      !from.is_empty() && !to.is_empty() && from != to,
      "invalid control link"
    );
    if allowed {
      diesel::insert_into(control_links)
        .values((source.eq(from), target.eq(to)))
        .on_conflict_do_nothing()
        .execute(&mut *self.lock()?)?;
    } else {
      diesel::delete(control_links.find((from, to))).execute(&mut *self.lock()?)?;
    }
    Ok(())
  }

  pub fn control_link_allowed(&self, from: &str, to: &str) -> Result<bool> {
    use self::control_links::dsl::*;
    Ok(
      control_links
        .find((from, to))
        .select(source)
        .first::<String>(&mut *self.lock()?)
        .optional()?
        .is_some(),
    )
  }
}
