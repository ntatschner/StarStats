//! Tiny helper bin: print the OpenAPI spec to stdout.
//!
//! Used by the TS codegen pipeline so we don't need a running
//! Postgres / SpiceDB / MinIO stack just to dump the spec. Imports
//! the same `openapi::ApiDoc` the live server serves at
//! `/openapi.json`.
//!
//! Usage:
//!   cargo run -p starstats-server --bin starstats-server-openapi > openapi.json

// The bin only consumes `ApiDoc::openapi()`; the rest of the modules
// exist solely so the macro derive sees the annotated handlers and
// schemas. Hence the dead-code blanket — every "unused" function is
// referenced by a different bin (the live server).
#![allow(dead_code)]
#![allow(unused_imports)]

// We re-declare the same module tree as `main.rs` because Cargo bins
// each have their own crate root. The compile cost is the same as
// the main bin's: utoipa's derive macros walk these modules to
// emit the schema.

#[path = "../admin_routes.rs"]
mod admin_routes;
#[path = "../admin_submission_routes.rs"]
mod admin_submission_routes;
#[path = "../api_error.rs"]
mod api_error;
#[path = "../audit.rs"]
mod audit;
#[path = "../audit_mirror.rs"]
mod audit_mirror;
#[path = "../auth.rs"]
mod auth;
#[path = "../auth_routes.rs"]
mod auth_routes;
#[path = "../config.rs"]
mod config;
#[path = "../device_routes.rs"]
mod device_routes;
#[path = "../devices.rs"]
mod devices;
#[path = "../hangar_routes.rs"]
mod hangar_routes;
#[path = "../hangar_store.rs"]
mod hangar_store;
#[path = "../health.rs"]
mod health;
#[path = "../ingest.rs"]
mod ingest;
#[path = "../kek.rs"]
mod kek;
#[path = "../locations.rs"]
mod locations;
#[path = "../magic_link.rs"]
mod magic_link;
#[path = "../magic_link_routes.rs"]
mod magic_link_routes;
#[path = "../mail.rs"]
mod mail;
#[path = "../openapi.rs"]
mod openapi;
#[path = "../orders.rs"]
mod orders;
#[path = "../org_routes.rs"]
mod org_routes;
#[path = "../orgs.rs"]
mod orgs;
#[path = "../preferences_routes.rs"]
mod preferences_routes;
#[path = "../preferences_store.rs"]
mod preferences_store;
#[path = "../profile_store.rs"]
mod profile_store;
#[path = "../query.rs"]
mod query;
#[path = "../recovery_codes.rs"]
mod recovery_codes;
#[path = "../reference_data.rs"]
mod reference_data;
#[path = "../reference_routes.rs"]
mod reference_routes;
#[path = "../reference_store.rs"]
mod reference_store;
#[path = "../repo.rs"]
mod repo;
#[path = "../revolut.rs"]
mod revolut;
#[path = "../revolut_routes.rs"]
mod revolut_routes;
#[path = "../rsi_org_routes.rs"]
mod rsi_org_routes;
#[path = "../rsi_org_store.rs"]
mod rsi_org_store;
#[path = "../rsi_profile_routes.rs"]
mod rsi_profile_routes;
#[path = "../rsi_verify.rs"]
mod rsi_verify;
#[path = "../rsi_verify_routes.rs"]
mod rsi_verify_routes;
#[path = "../sharing_routes.rs"]
mod sharing_routes;
#[path = "../smtp_admin_routes.rs"]
mod smtp_admin_routes;
#[path = "../smtp_config_store.rs"]
mod smtp_config_store;
#[path = "../spicedb.rs"]
mod spicedb;
#[path = "../staff_roles.rs"]
mod staff_roles;
#[path = "../submission_routes.rs"]
mod submission_routes;
#[path = "../submissions.rs"]
mod submissions;
#[path = "../supporter_routes.rs"]
mod supporter_routes;
#[path = "../supporters.rs"]
mod supporters;
#[path = "../telemetry.rs"]
mod telemetry;
#[path = "../totp.rs"]
mod totp;
#[path = "../totp_routes.rs"]
mod totp_routes;
#[path = "../update_routes.rs"]
mod update_routes;
#[path = "../users.rs"]
mod users;
#[path = "../validation.rs"]
mod validation;
#[path = "../well_known.rs"]
mod well_known;

use utoipa::OpenApi;

fn main() {
    let json = openapi::ApiDoc::openapi()
        .to_pretty_json()
        .expect("serialize OpenAPI spec");
    println!("{json}");
}
