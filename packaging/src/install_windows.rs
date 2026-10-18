//! Interactive entry point. MSI installs files elevated; the signed-in user
//! separately provisions their SCM identity using UAC and the native credential UI.
use agent_launch::{
  LaunchOptions, ensure_daemon,
  native::windows::{
    current_user_sid, elevate_service_setup, setup_is_elevated, validate_expected_user_sid,
  },
};
use anyhow::{Context, Result, ensure};

pub async fn register() -> Result<()> {
  let args: Vec<_> = std::env::args_os().skip(1).collect();
  if args.len() == 1 && args[0] == "--help" {
    println!(
      "agent-install --register-service\nRegisters the shared Windows SCM service for this user using the protected MSI-installed agentd. Accept UAC for this same Windows account, then enter its password in the native credential dialog. Switching to a different administrator account during UAC is unsupported and rejected. MSI installation alone does not register a user's service."
    );
    return Ok(());
  }
  ensure!(
    matches!(args.len(), 1 | 3) && args[0] == "--register-service",
    "use agent-install --register-service after installing the native MSI; Windows registration does not copy executables or accept credentials on the command line"
  );
  let expected = if args.len() == 3 {
    ensure!(
      args[1] == "--expected-user-sid",
      "unknown service setup option"
    );
    Some(
      args[2]
        .to_str()
        .context("invalid expected Windows user SID")?,
    )
  } else {
    None
  };
  // Validate the original user's SID before data-directory, credential, or SCM
  // operations. UAC over-the-shoulder elevation must not provision another user.
  if let Some(expected) = expected {
    validate_expected_user_sid(expected)?;
  }
  let sid = current_user_sid()?;
  if !setup_is_elevated()? {
    ensure!(
      expected.is_none(),
      "service setup helper did not receive an elevated token; no registration occurred"
    );
    let executable = std::env::current_exe().context("locate native service setup helper")?;
    let status = elevate_service_setup(&executable, &sid)?;
    ensure!(
      status == 0,
      "elevated Windows service setup failed with exit code {status}; retry the setup shortcut with the same Windows account"
    );
    return Ok(());
  }
  // An intentional direct administrator launch registers its current user.
  // Interactive elevation from an ordinary launch always carries the original SID.
  ensure_daemon(LaunchOptions::bundled(["sessions", "runs"])?).await?;
  println!("The shared Windows SCM agentd service is ready for this user.");
  Ok(())
}
