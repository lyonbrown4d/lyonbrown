use super::{Context, ObservationReference, input};
use agent_computer::{DesktopSnapshot, ObservationLimits};
use agent_computer_runtime::{Operation, Reply};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

impl Context {
  pub(super) async fn capture(
    &self,
    lease: agent_computer::LeaseToken,
    cancellation: CancellationToken,
  ) -> Result<DesktopSnapshot> {
    self.renew(&lease, cancellation.clone()).await?;
    let reply = self
      .request(
        Operation::Observe {
          lease,
          limits: ObservationLimits::default(),
          timeout_ms: 10_000,
        },
        cancellation,
      )
      .await?;
    let Reply::Snapshot(snapshot) = reply else {
      anyhow::bail!("invalid computer observation")
    };
    Ok(snapshot)
  }

  pub(super) async fn observe(
    &self,
    arguments: Value,
    cancellation: CancellationToken,
  ) -> Result<Value> {
    let input: input::Observe = serde_json::from_value(arguments)?;
    self.fence(&cancellation)?;
    let mut state = self.state.lock().await;
    state.observation = None;
    let lease = match &state.lease {
      Some(lease) => lease.clone(),
      None => {
        let Reply::Lease(lease) = self
          .request(
            Operation::AcquireLease {
              ttl_ms: self.lease_ms,
            },
            cancellation.clone(),
          )
          .await?
        else {
          anyhow::bail!("invalid computer lease");
        };
        state.lease = Some(lease.clone());
        self.lease_watch.send_replace(Some(lease.clone()));
        lease
      }
    };
    let snapshot = self.capture(lease, cancellation.clone()).await?;
    let path = self.daemon.config.data_path()?.join("artifacts");
    let node = self.daemon.node_id.clone();
    let (reference, output) = tokio::task::spawn_blocking(move || -> Result<_> {
      let reference = ObservationReference {
        id: snapshot.id,
        revision: snapshot.revision,
        digest: snapshot.content_digest()?,
      };
      let observation = snapshot.observation;
      let mut output = json!({
        "snapshot_id":snapshot.id,"revision":snapshot.revision,"scope":snapshot.session.scope,
        "bounds":observation.bounds,"windows":observation.windows,
        "windows_truncated":observation.windows_truncated,"accessibility":observation.accessibility,
        "screenshot_artifact":""
      });
      ensure!(
        serde_json::to_vec(&output)?.len() <= 1024 * 1024,
        "computer semantics exceed limit"
      );
      if input.screenshot {
        use image::ImageEncoder;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
          &observation.screenshot.rgba,
          observation.screenshot.width,
          observation.screenshot.height,
          image::ExtendedColorType::Rgba8,
        )?;
        let artifact =
          agent_workspace::ArtifactStore::open(&path, &node)?.put(&png, "image/png")?;
        output["screenshot_artifact"] = json!(artifact.digest);
      }
      Ok((
        reference,
        json!({"untrusted_desktop_content":true,"observation":output}),
      ))
    })
    .await??;
    self.fence(&cancellation)?;
    state.observation = Some(reference);
    Ok(output)
  }
}
