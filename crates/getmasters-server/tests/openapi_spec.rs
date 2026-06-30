//! Guards that the OpenAPI document builds and includes the Phase 0 surface. The emitted
//! `openapi.json` (via the `gen_openapi` binary) is the source for the TypeScript client, so
//! a broken spec must fail the build, not silently ship.

use getmasters_server::ApiDoc;
use utoipa::OpenApi;

#[test]
fn openapi_serializes_and_covers_phase0_surface() {
    let doc = ApiDoc::openapi();
    let json = doc.to_pretty_json().expect("spec serializes to JSON");

    for path in ["/health", "/sessions", "/sessions/{id}/messages"] {
        assert!(json.contains(path), "spec missing path {path}");
    }
    // DTOs + WS envelopes must be present as components for the TS client.
    for schema in ["SessionDto", "MessageDto", "ServerEvent", "ClientCommand"] {
        assert!(json.contains(schema), "spec missing schema {schema}");
    }
}
