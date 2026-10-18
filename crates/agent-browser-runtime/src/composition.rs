use fluxdi::{Error, Shared, static_module};
use std::path::PathBuf;

pub struct RuntimeConfiguration {
  pub profiles: PathBuf,
}
pub struct ProfileRoot(pub PathBuf);

fn profile_root(configuration: Shared<RuntimeConfiguration>) -> Result<Shared<ProfileRoot>, Error> {
  let prepare = || -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(&configuration.profiles)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(
        &configuration.profiles,
        std::fs::Permissions::from_mode(0o700),
      )?;
    }
    configuration.profiles.canonicalize()
  };
  prepare()
    .map(|path| Shared::new(ProfileRoot(path)))
    .map_err(|error| Error::factory_failed("BrowserProfileRoot", &error.to_string()))
}

static_module! {
  pub struct RuntimeModule {
    external configuration: RuntimeConfiguration;
    #[scope(root)]
    profiles: ProfileRoot = profile_root(configuration);
  }
}
