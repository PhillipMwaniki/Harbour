//! Host storage and imports.
//!
//! The store is SQLite ([`store`]); the secrets that go with it are in the OS
//! keychain ([`secrets`]) and never in the database. [`ssh_config`] and
//! [`xshell`] read other tools' formats so an existing estate can be brought
//! across without retyping it.

pub mod export;
pub mod import;
pub mod model;
pub mod secrets;
pub mod ssh_config;
pub mod store;
pub mod xshell;
