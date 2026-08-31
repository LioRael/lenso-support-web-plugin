# Support Agent Workspace Plugin card

- **Job:** let an authenticated support agent list and open cases, reply publicly, collaborate with internal notes, assign ownership, and transition state without moving case facts or visibility policy into the Web layer.
- **Provides:** `lenso.http.endpoint@1` (`describe`, `handle`).
- **Requires:** exactly one `lenso.auth@1` and one `lenso.support-case@1`.
- **Owns:** route descriptions, static page assets, typed HTTP decoding, authentication evidence selection, ActorAssertion forwarding, and intentional HTTP error representation.
- **Does not own:** cases, messages, requester identity, agent assignment, internal-note visibility, transitions, revisions, idempotency, authorization, or persistence.
- **Visibility rule:** target `forbidden` and `case_not_found` outcomes are both represented as `404`; the browser never infers hidden facts.
- **Success proof:** a real Kernel composition demonstrates that the Support Provider verifies the forwarded actor assertion.
- **Lifecycle and state:** stateless; no owned resource or managed task is prepared for a Generation.
- **First observable behavior:** an authenticated agent loads `/support`, opens `SUP-1`, replies, and moves it to `waiting_customer`; invalid credentials return `401` and a stale revision returns `409`.
- **Deletion proof:** `removing_agent_workspace_does_not_remove_support_provider` resolves and starts a Plan without `lenso.support.web`, then invokes `lenso.support-case@1` successfully. Deletion removes only the agent page and routes; Support Case facts and non-Web consumers remain.
- **Host boundary:** a native Host must link this crate and bind it to Web Ingress. It is not automatically installed by generic `lenso run`.
- **Console boundary:** the current Console has no UI-contribution contract; this Plugin therefore serves `/support` but cannot truthfully self-register Console navigation.
