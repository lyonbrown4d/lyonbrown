//! Zero-configuration workspace detection and canonical repository instructions.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const INSTRUCTION_FILES: [&str; 4] = ["AGENTS.md", "CLAUDE.md", "README.md", "CONTRIBUTING.md"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectType {
    Rust,
    Go,
    Maven,
    Gradle,
    Node,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryInstruction {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProfile {
    pub root: PathBuf,
    pub project_type: ProjectType,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub build_commands: Vec<String>,
    pub test_commands: Vec<String>,
    pub conventions: Vec<String>,
    pub enabled_skills: Vec<String>,
    pub repository_instructions: Vec<RepositoryInstruction>,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace path {path} is not a directory")]
    NotDirectory { path: PathBuf },
    #[error("could not read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl WorkspaceProfile {
    pub fn detect(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(WorkspaceError::NotDirectory {
                path: root.to_path_buf(),
            });
        }
        let (project_type, languages, build_commands, test_commands) = detect_manifest(root);
        Ok(Self {
            root: root.to_path_buf(),
            project_type,
            languages,
            frameworks: Vec::new(),
            build_commands,
            test_commands,
            conventions: Vec::new(),
            enabled_skills: Vec::new(),
            repository_instructions: load_repository_instructions(root)?,
        })
    }
}

pub fn load_repository_instructions(
    root: impl AsRef<Path>,
) -> Result<Vec<RepositoryInstruction>, WorkspaceError> {
    let root = root.as_ref();
    INSTRUCTION_FILES
        .iter()
        .filter_map(|name| {
            let path = root.join(name);
            path.is_file().then_some((*name, path))
        })
        .map(|(name, path)| {
            fs::read_to_string(&path)
                .map(|content| RepositoryInstruction {
                    path: name.into(),
                    content,
                })
                .map_err(|source| WorkspaceError::Io { path, source })
        })
        .collect()
}

pub fn workspace_cache_path(
    home_root: impl AsRef<Path>,
    workspace_root: impl AsRef<Path>,
) -> Result<PathBuf, WorkspaceError> {
    let workspace_root = workspace_root.as_ref();
    let identity = workspace_root
        .canonicalize()
        .map_err(|source| WorkspaceError::Io {
            path: workspace_root.to_path_buf(),
            source,
        })?;
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in identity.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(home_root
        .as_ref()
        .join(".agent")
        .join("workspaces")
        .join(format!("{hash:016x}")))
}

fn detect_manifest(root: &Path) -> (ProjectType, Vec<String>, Vec<String>, Vec<String>) {
    if root.join("Cargo.toml").is_file() {
        return (
            ProjectType::Rust,
            vec!["Rust".into()],
            vec!["cargo build".into()],
            vec!["cargo test".into()],
        );
    }
    if root.join("go.mod").is_file() {
        return (
            ProjectType::Go,
            vec!["Go".into()],
            vec!["go build ./...".into()],
            vec!["go test ./...".into()],
        );
    }
    if root.join("pom.xml").is_file() {
        return (
            ProjectType::Maven,
            vec!["Java".into()],
            vec!["mvn package".into()],
            vec!["mvn test".into()],
        );
    }
    if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        return (
            ProjectType::Gradle,
            vec!["Java".into()],
            vec!["./gradlew build".into()],
            vec!["./gradlew test".into()],
        );
    }
    if root.join("package.json").is_file() {
        return (
            ProjectType::Node,
            vec!["JavaScript".into()],
            vec!["npm run build".into()],
            vec!["npm test".into()],
        );
    }
    (ProjectType::Unknown, Vec::new(), Vec::new(), Vec::new())
}
