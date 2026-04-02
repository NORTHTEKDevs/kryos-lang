//! Kryos package manager — kryos.toml manifests, semantic versioning,
//! dependency resolution, and lock file management.

pub mod semver;
pub mod manifest;
pub mod resolve;
pub mod lock;

pub use manifest::{Manifest, PackageInfo, DepSpec, CapabilitiesConfig, BuildConfig};
pub use semver::{Version, VersionReq, Op};
pub use resolve::{resolve, ResolvedGraph, ResolvedPackage, PackageSource, PackageRegistry, AvailablePackage, ResolveError};
pub use lock::{LockFile, LockEntry};
