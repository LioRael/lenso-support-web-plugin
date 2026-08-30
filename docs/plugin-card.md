# Support Web Plugin card

- **Job:** operate a support inbox without moving case facts or visibility policy into the Web layer.
- **Provides:** `lenso.http.endpoint@1` (`describe`, `handle`).
- **Requires:** exactly one `lenso.auth@1` and one `lenso.support-case@1`.
- **Owns:** route descriptions, static page assets, typed HTTP decoding, authentication evidence selection, ActorAssertion forwarding, and intentional HTTP error representation.
- **Does not own:** cases, messages, requester identity, agent assignment, internal-note visibility, transitions, revisions, idempotency, authorization, or persistence.
- **Visibility rule:** target `forbidden` and `case_not_found` outcomes are both represented as `404`; the browser never infers hidden facts.
- **Success proof:** a real Kernel composition demonstrates that the Support Provider verifies the forwarded actor assertion.
- **Deletion proof:** a resolved Plan without `lenso.support.web` still starts and invokes `lenso.support-case@1` successfully.
- **Host boundary:** a native Host must link this crate and bind it to Web Ingress. It is not automatically installed by generic `lenso run`.
- **Console boundary:** the current Console has no UI-contribution contract; this Plugin therefore serves `/support` but cannot truthfully self-register Console navigation.
