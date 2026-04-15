//! Kryos package manager — kryos.toml manifests, semantic versioning,
//! dependency resolution, and lock file management.

#![allow(clippy::should_implement_trait, clippy::too_many_arguments)]

pub mod fetch;
pub mod lock;
pub mod manifest;
pub mod registry;
pub mod resolve;
pub mod semver;

pub use lock::{LockEntry, LockFile};
pub use manifest::{BuildConfig, CapabilitiesConfig, DepSpec, Manifest, PackageInfo};
pub use registry::{RegistryClient, RegistryConfig, RegistryEntry};
pub use resolve::{
    resolve, AvailablePackage, PackageRegistry, PackageSource, ResolveError, ResolvedGraph,
    ResolvedPackage,
};
pub use semver::{Op, Version, VersionReq};
