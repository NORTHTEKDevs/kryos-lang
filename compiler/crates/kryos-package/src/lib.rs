//! Kryos package manager — kryos.toml manifests, semantic versioning,
//! dependency resolution, and lock file management.

#![allow(clippy::should_implement_trait, clippy::too_many_arguments)]

pub mod semver;
pub mod manifest;
pub mod resolve;
pub mod lock;
pub mod registry;
pub mod fetch;

pub use manifest::{Manifest, PackageInfo, DepSpec, CapabilitiesConfig, BuildConfig};
pub use semver::{Version, VersionReq, Op};
pub use resolve::{resolve, ResolvedGraph, ResolvedPackage, PackageSource, PackageRegistry, AvailablePackage, ResolveError};
pub use lock::{LockFile, LockEntry};
pub use registry::{RegistryClient, RegistryConfig, RegistryEntry};
