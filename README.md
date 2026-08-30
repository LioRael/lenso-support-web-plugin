# Lenso Support Web Plugin

`lenso.support.web` is a removable, linked native Web Plugin for the Lenso Support Case workflow. It provides `lenso.http.endpoint@1` and delegates every case, transition, assignment, public message, and internal note to exactly one bound `lenso.support-case@1` Provider.

## Product workflow

The page at `/support` provides a filterable, cursor-paginated case inbox; case detail and conversation; case creation; state transitions; public replies; and internal notes. The typed JSON API additionally exposes case update and assignment operations.

The Web Plugin does not persist, filter, or independently reconstruct cases or messages. It authenticates selected ingress evidence through exactly one `lenso.auth@1`, attaches the returned `ActorAssertion`, and invokes the Support Case Provider with `_with_context`. The target remains responsible for requester-versus-agent visibility, internal-note access, valid transitions, revision checks, idempotency, and persistence.

## Host linking and ingress

This crate is a linked native Plugin, not a portable Bundle and not a standalone HTTP server. A Host must:

1. link the crate (calling `lenso_support_web_plugin::link()` is a convenient retention reference);
2. build its registry with `NativePluginRegistry::with_linked_factories()`;
3. place `lenso.support.web` in the resolved App Plan;
4. bind its one Auth and one Support Case requirement;
5. bind the Host's Web Ingress `many lenso.http.endpoint@1` requirement to this Plugin.

The generic `lenso run` flow does not distribute or link arbitrary native Web Plugins. The current Console also has no `lenso.ui.contribution@1` or `lenso.web.shell@1` contract, so installing this crate does **not** add a Console navigation item. It serves an honest standalone `/support` surface when a Host links and routes it. Console embedding remains a separate platform prerequisite.

## HTTP behavior

All errors use `application/problem+json`. Missing or invalid credentials produce `401`; a wrong actor kind produces `403`; both forbidden and absent cases map to visibility-safe `404`; revision, idempotency, and invalid-transition conflicts produce `409`; invalid requests produce `400`. A Runtime Failure from Auth or Support Case crosses the Endpoint boundary unchanged so Web Ingress can apply its infrastructure policy.

The `/messages` write route accepts only `public`, while `/notes` accepts only `internal`. This route-level distinction improves intent, but the target Provider still decides whether the actor may read or create either visibility.

Static HTML, CSS, and JavaScript are embedded in the crate. The browser stores the organization and bearer credential only in its local storage; production Hosts should normally supply credentials through their own secure session ingress policy.

## Verification

```bash
/Users/leosouthey/Projects/framework/.lenso-tools/bin/lenso-cargo fmt --all -- --check
/Users/leosouthey/Projects/framework/.lenso-tools/bin/lenso-cargo check --locked --workspace --all-targets
/Users/leosouthey/Projects/framework/.lenso-tools/bin/lenso-cargo test --locked --workspace
/Users/leosouthey/Projects/framework/.lenso-tools/bin/lenso-cargo clippy --locked --workspace --all-targets -- -D warnings
./scripts/check-repository-boundary.sh
./scripts/check-public-packages.sh
```
