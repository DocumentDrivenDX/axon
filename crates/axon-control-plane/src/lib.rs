//! Axon control plane (FEAT-025).
//!
//! A lightweight management plane for multi-tenant Axon deployments. Provides
//! centralized tenant lifecycle management, monitoring, and operational
//! visibility. Designed for the BYOC (Bring Your Own Cloud) commercial model
//! where customers run Axon in their own infrastructure.
//!
//! # Data sovereignty
//!
//! The control plane *never* reads or stores customer entity data. It tracks
//! only metadata: tenant identity, backing-store configuration, deployment
//! mode, last-seen timestamps, and aggregated health samples supplied by the
//! tenant instance itself. All richer metrics must be exposed through a
//! metrics endpoint that the control plane scrapes, not by inspection of the
//! tenant's database.
//!
//! # Layout
//!
//! - [`model`] — tenant data model, lifecycle, and health types.
//! - [`store`] — [`ControlPlaneStore`] trait + in-memory implementation.
//! - [`service`] — business logic for tenant lifecycle and registration.
//! - [`http`] — axum HTTP router exposing the control plane API.
//! - [`error`] — control plane error hierarchy.

pub mod error;
pub mod http;
pub mod model;
pub mod service;
pub mod store;

pub use error::ControlPlaneError;
pub use model::{
    BackingStore, DataRetentionPolicy, DeploymentMode, HealthReport, HealthStatus, Tenant,
    TenantId, TenantSpec, TenantStatus,
};
pub use service::ControlPlaneService;
pub use store::{ControlPlaneStore, InMemoryControlPlaneStore};
