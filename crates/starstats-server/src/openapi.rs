//! OpenAPI spec assembly + JSON serving route.
//!
//! `ApiDoc` lists every annotated handler and every type that needs
//! to surface in `components.schemas`. The macro pulls the
//! `#[utoipa::path(...)]` block off each handler and stitches them
//! together; we keep it intentionally manual (no `utoipa-axum`) so a
//! future utoipa minor doesn't yank the rug out — the worst that
//! happens is a stale entry on this list.
//!
//! The spec is exposed at `GET /openapi.json` for clients that want
//! to fetch live; the same spec is dumped to stdout by the
//! `starstats-server-openapi` bin for offline TS codegen.

use crate::admin_org_routes;
use crate::admin_reference_routes;
use crate::admin_routes;
use crate::admin_sharing_routes;
use crate::admin_submission_routes;
use crate::admin_user_routes;
use crate::api_error;
use crate::auth_routes;
use crate::device_routes;
use crate::hangar_routes;
use crate::hangar_store;
use crate::health;
use crate::ingest;
use crate::magic_link_routes;
use crate::org_routes;
use crate::preferences_routes;
use crate::query;
use crate::reference_data;
use crate::reference_routes;
use crate::revolut_routes;
use crate::rsi_org_routes;
use crate::rsi_org_store;
use crate::rsi_profile_routes;
use crate::rsi_verify;
use crate::rsi_verify_routes;
use crate::sharing_routes;
use crate::smtp_admin_routes;
use crate::submission_routes;
use crate::supporter_routes;
use crate::totp_routes;
use crate::update_routes;
use crate::well_known;
use axum::{response::IntoResponse, routing::get, Router};
use utoipa::{
    openapi::security::{Http, HttpAuthScheme, SecurityScheme},
    Modify, OpenApi,
};

/// Modifier that injects the single `BearerAuth` scheme that protected
/// handlers reference via `security(("BearerAuth" = []))`.
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::new);
        components.add_security_scheme(
            "BearerAuth",
            SecurityScheme::Http(
                Http::builder()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "StarStats API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Self-hosted ingest + read API for StarStats. \
                       All `/v1/*` routes besides `/v1/auth/login`, \
                       `/v1/auth/signup`, `/v1/auth/email/verify`, and \
                       `/v1/auth/devices/redeem` require a bearer token.",
    ),
    paths(
        health::live,
        health::ready,
        health::metrics,
        well_known::jwks,
        well_known::openid_configuration,
        ingest::handle,
        query::list_events,
        query::hide_event,
        query::unhide_event,
        query::summary,
        query::timeline,
        query::metrics_event_types,
        query::metrics_sessions,
        query::ingest_history,
        query::location_current,
        query::location_trace,
        query::location_breakdown,
        query::stats_combat,
        query::stats_travel,
        query::stats_loadout,
        query::stats_stability,
        auth_routes::signup,
        auth_routes::login,
        auth_routes::verify_email,
        auth_routes::change_password,
        auth_routes::resend_verification,
        auth_routes::delete_account,
        auth_routes::get_me,
        auth_routes::password_reset_start,
        auth_routes::password_reset_complete,
        auth_routes::email_change_start,
        auth_routes::email_change_verify,
        device_routes::start,
        device_routes::list,
        device_routes::revoke,
        device_routes::redeem,
        rsi_verify_routes::start,
        rsi_verify_routes::verify,
        rsi_profile_routes::refresh,
        rsi_profile_routes::me,
        rsi_profile_routes::public_profile,
        rsi_org_routes::refresh,
        rsi_org_routes::me,
        rsi_org_routes::public_orgs,
        hangar_routes::push,
        hangar_routes::me,
        preferences_routes::get,
        preferences_routes::put,
        magic_link_routes::start,
        magic_link_routes::redeem,
        totp_routes::setup,
        totp_routes::confirm,
        totp_routes::disable,
        totp_routes::regenerate_recovery,
        totp_routes::verify_login,
        sharing_routes::set_visibility,
        sharing_routes::get_visibility,
        sharing_routes::add_share,
        sharing_routes::delete_share,
        sharing_routes::list_shares,
        sharing_routes::list_shared_with_me,
        sharing_routes::share_with_org,
        sharing_routes::unshare_with_org,
        sharing_routes::public_summary,
        sharing_routes::public_timeline,
        sharing_routes::friend_summary,
        sharing_routes::friend_timeline,
        org_routes::create_org,
        org_routes::list_orgs,
        org_routes::get_org,
        org_routes::delete_org,
        org_routes::add_member,
        org_routes::remove_member,
        update_routes::check_for_update,
        reference_routes::list_vehicles,
        reference_routes::get_vehicle,
        reference_routes::list_entries,
        reference_routes::get_entry,
        submission_routes::list,
        submission_routes::create,
        submission_routes::detail,
        submission_routes::vote,
        submission_routes::flag,
        submission_routes::withdraw,
        admin_routes::list_audit,
        admin_sharing_routes::get_overview,
        admin_sharing_routes::get_scope_histogram,
        admin_submission_routes::accept,
        admin_submission_routes::reject,
        admin_submission_routes::dismiss_flag,
        admin_submission_routes::queue,
        admin_user_routes::list_users_admin::<crate::users::PostgresUserStore>,
        admin_user_routes::get_user_admin::<crate::users::PostgresUserStore>,
        admin_user_routes::grant_role::<crate::users::PostgresUserStore>,
        admin_user_routes::revoke_role::<crate::users::PostgresUserStore>,
        admin_org_routes::list_orgs_admin::<crate::orgs::PostgresOrgStore>,
        admin_org_routes::get_org_admin::<crate::orgs::PostgresOrgStore>,
        admin_org_routes::delete_org_admin::<crate::orgs::PostgresOrgStore>,
        admin_reference_routes::list_reference_categories::<crate::reference_store::PostgresReferenceStore>,
        admin_reference_routes::list_reference_entries::<crate::reference_store::PostgresReferenceStore>,
        smtp_admin_routes::get_smtp,
        smtp_admin_routes::put_smtp,
        smtp_admin_routes::test_smtp,
        supporter_routes::get_me,
        revolut_routes::list_tiers,
        revolut_routes::checkout,
        revolut_routes::webhook,
    ),
    components(schemas(
        // Shared error envelope (single canonical type for all routes)
        api_error::ApiErrorBody,
        // Health
        health::HealthResponseSchema,
        health::ReadyResponseSchema,
        health::ReadyChecksSchema,
        // Well-known
        well_known::JwksDocument,
        well_known::Jwk,
        well_known::OidcDiscovery,
        // Ingest
        ingest::IngestResponse,
        ingest::IngestBatchSchema,
        ingest::EventEnvelopeSchema,
        // Query
        query::EventsListResponse,
        query::EventDto,
        query::HideToggleResponse,
        query::SummaryResponse,
        query::TypeCount,
        query::TimelineResponse,
        query::TimelineBucket,
        query::EventTypeBreakdownResponse,
        query::EventTypeStatsDto,
        query::SessionsResponse,
        query::SessionDto,
        query::IngestHistoryResponse,
        query::IngestBatchDto,
        query::CurrentLocationResponse,
        crate::locations::ResolvedLocation,
        query::TraceResponse,
        query::TraceEntry,
        query::BreakdownResponse,
        query::BreakdownEntry,
        query::StatsBucket,
        query::CombatStatsResponse,
        query::TravelStatsResponse,
        query::LoadoutStatsResponse,
        query::StabilityStatsResponse,
        query::CommerceRecentResponse,
        query::CommerceTransactionDto,
        // Submissions
        submission_routes::SubmissionDto,
        submission_routes::ListResponse,
        submission_routes::CreateSubmissionRequest,
        submission_routes::CreateSubmissionResponse,
        submission_routes::VoteRequest,
        submission_routes::VoteResponse,
        submission_routes::FlagRequest,
        submission_routes::FlagResponse,
        submission_routes::WithdrawResponse,
        // Admin submission moderation
        admin_routes::AuditEntryDto,
        admin_routes::AuditListResponse,
        // Admin sharing overview
        admin_sharing_routes::AdminSharingOverview,
        admin_sharing_routes::TopGranter,
        admin_sharing_routes::ScopeHistogram,
        admin_user_routes::AdminUserDto,
        admin_user_routes::AdminUserListResponse,
        admin_user_routes::GrantRoleRequest,
        admin_user_routes::RoleTransitionResponse,
        admin_org_routes::AdminOrgDto,
        admin_org_routes::AdminOrgListResponse,
        admin_org_routes::AdminOrgDeleteResponse,
        admin_reference_routes::AdminReferenceCategoryDto,
        admin_reference_routes::AdminReferenceCategoriesResponse,
        admin_reference_routes::AdminReferenceEntryDto,
        admin_reference_routes::AdminReferenceEntriesResponse,
        admin_submission_routes::SubmissionTransitionResponse,
        admin_submission_routes::RejectRequest,
        admin_submission_routes::AdminQueueResponse,
        // Admin SMTP config
        smtp_admin_routes::SmtpConfigResponse,
        smtp_admin_routes::SmtpConfigRequest,
        smtp_admin_routes::TestSendResponse,
        smtp_admin_routes::TestSendRequest,
        // Supporter (donate) status
        supporter_routes::SupporterStatusDto,
        // Donate / Revolut
        revolut_routes::TierDto,
        revolut_routes::TierListResponse,
        revolut_routes::CheckoutRequest,
        revolut_routes::CheckoutResponse,
        revolut_routes::WebhookAck,
        // Auth
        auth_routes::SignupRequest,
        auth_routes::LoginRequest,
        auth_routes::AuthResponse,
        auth_routes::VerifyEmailRequest,
        auth_routes::VerifyEmailResponse,
        auth_routes::ChangePasswordRequest,
        auth_routes::ChangePasswordResponse,
        auth_routes::ResendVerificationResponse,
        auth_routes::DeleteAccountRequest,
        auth_routes::DeleteAccountResponse,
        auth_routes::MeResponse,
        auth_routes::PasswordResetStartRequest,
        auth_routes::PasswordResetStartResponse,
        auth_routes::PasswordResetCompleteRequest,
        auth_routes::PasswordResetCompleteResponse,
        auth_routes::EmailChangeStartRequest,
        auth_routes::EmailChangeStartResponse,
        auth_routes::EmailChangeVerifyRequest,
        auth_routes::EmailChangeVerifyResponse,
        // Devices
        device_routes::StartRequest,
        device_routes::StartResponse,
        device_routes::RedeemRequest,
        device_routes::RedeemResponse,
        device_routes::DeviceListResponse,
        device_routes::DeviceDto,
        // RSI verify
        rsi_verify_routes::RsiStartResponse,
        rsi_verify_routes::RsiVerifyResponse,
        // RSI profile
        rsi_profile_routes::ProfileResponse,
        rsi_verify::Badge,
        // RSI orgs
        rsi_verify::RsiOrg,
        rsi_org_store::RsiOrgsSnapshot,
        // Hangar
        hangar_store::HangarSnapshot,
        hangar_routes::HangarPushRequestSchema,
        hangar_routes::HangarShipSchema,
        // Preferences
        preferences_routes::UserPreferencesSchema,
        // Magic link
        magic_link_routes::MagicLinkStartRequest,
        magic_link_routes::MagicLinkStartResponse,
        magic_link_routes::MagicLinkRedeemRequest,
        // TOTP
        totp_routes::TotpSetupResponse,
        totp_routes::TotpConfirmRequest,
        totp_routes::TotpConfirmResponse,
        totp_routes::TotpDisableRequest,
        totp_routes::TotpDisableResponse,
        totp_routes::RegenerateRecoveryRequest,
        totp_routes::RegenerateRecoveryResponse,
        totp_routes::VerifyLoginRequest,
        // Sharing
        sharing_routes::VisibilityRequest,
        sharing_routes::VisibilityResponse,
        sharing_routes::ShareScope,
        sharing_routes::ShareRequest,
        sharing_routes::ShareResponse,
        sharing_routes::RevokeShareResponse,
        sharing_routes::ShareEntry,
        sharing_routes::OrgShareEntry,
        sharing_routes::ShareOrgRequest,
        sharing_routes::ShareOrgResponse,
        sharing_routes::RevokeOrgShareResponse,
        sharing_routes::ListSharesResponse,
        sharing_routes::SharedWithMeEntry,
        sharing_routes::ListSharedWithMeResponse,
        sharing_routes::PublicSummaryResponse,
        sharing_routes::PublicTypeCount,
        sharing_routes::PublicTimelineResponse,
        sharing_routes::PublicTimelineBucket,
        // Orgs
        org_routes::CreateOrgRequest,
        org_routes::CreateOrgResponse,
        org_routes::OrgDto,
        org_routes::ListOrgsResponse,
        org_routes::OrgMemberDto,
        org_routes::GetOrgResponse,
        org_routes::DeleteOrgResponse,
        org_routes::AddMemberRequest,
        org_routes::AddMemberResponse,
        org_routes::RemoveMemberResponse,
        // Updater
        update_routes::UpdateManifest,
        update_routes::PlatformBundle,
        // Reference data
        reference_data::VehicleReference,
        reference_routes::VehicleListResponse,
        reference_data::ReferenceCategory,
        reference_data::ReferenceEntry,
        reference_routes::ReferenceListResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness, readiness, metrics"),
        (name = "well-known", description = "JWKS + OIDC discovery"),
        (name = "auth", description = "Email + password account flow"),
        (name = "devices", description = "Device pairing flow"),
        (name = "rsi-verify", description = "RSI handle ownership verification via public bio"),
        (name = "rsi-profile", description = "Public RSI citizen profile snapshots"),
        (name = "rsi-orgs", description = "User's RSI organisation memberships"),
        (name = "hangar", description = "User-owned ship hangar snapshots"),
        (name = "preferences", description = "Per-user UI preferences (theme, etc.)"),
        (name = "totp", description = "TOTP 2FA setup, verification, and recovery codes"),
        (name = "ingest", description = "Client → server event batches"),
        (name = "query", description = "Read-side per-user query API"),
        (name = "sharing", description = "Public visibility + per-user share management"),
        (name = "orgs", description = "Organizations + membership"),
        (name = "updater", description = "Tauri auto-update manifest"),
        (name = "reference", description = "Star Citizen vehicle/item reference data (community-API-sourced)"),
        (name = "supporter", description = "Donate-status surface (read-only)"),
        (name = "donate", description = "Revolut hosted-checkout donate flow"),
        (name = "admin", description = "Site-wide staff endpoints (moderator/admin role required)"),
    )
)]
pub struct ApiDoc;

async fn openapi_json() -> impl IntoResponse {
    // `axum::Json` over the spec works, but utoipa's `to_pretty_json`
    // produces stable key ordering which matters for our drift-detection
    // CI step. Wrap in axum's IntoResponse via tuple to set the content
    // type explicitly.
    let body = ApiDoc::openapi()
        .to_json()
        .unwrap_or_else(|_| "{}".to_string());
    ([("content-type", "application/json")], body)
}

/// Returns a router exposing `GET /openapi.json` only. Merge into the
/// main router via `.merge(openapi::router())`.
pub fn router() -> Router {
    Router::new().route("/openapi.json", get(openapi_json))
}
