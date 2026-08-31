use std::{any::Any, cell::Cell, collections::BTreeMap, rc::Rc, time::Duration as StdDuration};

use futures::future::LocalBoxFuture;
use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    PluginInstancePlan, ResolvedAppPlan,
};
use lenso_auth_sdk::{
    ActorAssertion, ActorAssertionIssuer, ActorProjectionError, FixedClock, TypedActor, Validity,
    audience, authenticated_response,
};
use lenso_capability_auth as auth;
use lenso_capability_auth::{Auth, AuthEndpoint, AuthProvider};
use lenso_capability_http_endpoint as endpoint;
use lenso_capability_http_endpoint::{
    HandleRequest, HandleRequestCredential, HandleRequestHeadersItem,
    HandleRequestPathParametersItem,
};
use lenso_capability_support_case as support;
use lenso_kernel::{
    InvocationContext, Kernel, NativeRequestEndpoint, NativeRequestFuture, RuntimeFailure,
    ShutdownOutcome,
};
use lenso_native_adapter::{
    NativePluginFactory, NativePluginFactoryContext, NativePluginInstance, NativePluginRegistry,
};
use lenso_runner::TokioDriver;
use lenso_support_web_plugin::PACKAGE_ID;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

const CALLER_PACKAGE: &str = "test.support-web-caller";
const AUTH_PACKAGE: &str = "test.support-web-auth";
const DOMAIN_PACKAGE: &str = "test.support-web-domain";
const NOW: &str = "2027-01-15T08:00:00Z";

type EndpointResult = Result<Result<Box<dyn Any>, Box<dyn Any>>, RuntimeFailure>;
type EndpointFuture = LocalBoxFuture<'static, EndpointResult>;

const SUPPORT_OPERATIONS: &[&str] = &[
    support::ADD_MESSAGE_OPERATION,
    support::ASSIGN_CASE_OPERATION,
    support::CREATE_CASE_OPERATION,
    support::GET_CASE_OPERATION,
    support::LIST_CASES_OPERATION,
    support::LIST_MESSAGES_OPERATION,
    support::TRANSITION_CASE_OPERATION,
    support::UPDATE_CASE_OPERATION,
];

#[derive(Clone, Copy, Debug)]
enum SupportMode {
    Success,
    Forbidden,
    Runtime,
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_agent_can_list_open_reply_and_transition_with_conflicts_preserved() {
    tokio::task::LocalSet::new()
        .run_until(async {
            lenso_support_web_plugin::link();
            let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
            let issuer = ActorAssertionIssuer::new("test.auth", b"support-web-test-key");
            let observed_actor = Rc::new(Cell::new(false));
            let app = start_web_app(
                issuer.clone(),
                now,
                Rc::clone(&observed_actor),
                SupportMode::Success,
            )
            .await;

            let invalid_credential = app
                .invoke::<endpoint::EndpointHandle>(
                    "caller",
                    endpoint::HANDLE_OPERATION,
                    list_request("bad"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(invalid_credential.status, 401);
            assert!(body_text(&invalid_credential).contains("authentication_required"));

            let listed = app
                .invoke::<endpoint::EndpointHandle>(
                    "caller",
                    endpoint::HANDLE_OPERATION,
                    list_request("good"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(listed.status, 200);
            let listed_body = body_json(&listed);
            assert_eq!(listed_body["cases"][0]["identifier"], "SUP-1");
            assert_eq!(listed_body["cases"][0]["revision"], "rev-1");

            let opened = app
                .invoke::<endpoint::EndpointHandle>(
                    "caller",
                    endpoint::HANDLE_OPERATION,
                    detail_request("good"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(opened.status, 200);
            assert_eq!(body_json(&opened)["case_id"], "case_1");

            let replied = app
                .invoke::<endpoint::EndpointHandle>(
                    "caller",
                    endpoint::HANDLE_OPERATION,
                    public_reply_request("good", "rev-1"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(replied.status, 201);
            let replied_body = body_json(&replied);
            assert_eq!(replied_body["visibility"], "public");
            assert_eq!(replied_body["case_revision"], "rev-2");

            let transitioned = app
                .invoke::<endpoint::EndpointHandle>(
                    "caller",
                    endpoint::HANDLE_OPERATION,
                    transition_request("good", "rev-2"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(transitioned.status, 200);
            let transitioned_body = body_json(&transitioned);
            assert_eq!(transitioned_body["state"], "waiting_customer");
            assert_eq!(transitioned_body["revision"], "rev-3");

            let stale_transition = app
                .invoke::<endpoint::EndpointHandle>(
                    "caller",
                    endpoint::HANDLE_OPERATION,
                    transition_request("good", "rev-stale"),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(stale_transition.status, 409);
            assert!(body_text(&stale_transition).contains("revision_conflict"));
            assert!(observed_actor.get());

            assert_eq!(
                app.shutdown(StdDuration::from_secs(1)).await,
                ShutdownOutcome::Clean
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn preserves_visibility_safe_domain_errors_and_runtime_failures() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
            let issuer = ActorAssertionIssuer::new("test.auth", b"support-web-test-key");

            for (mode, expected_status) in [
                (SupportMode::Forbidden, Some(404)),
                (SupportMode::Runtime, None),
            ] {
                let app = start_web_app(issuer.clone(), now, Rc::new(Cell::new(false)), mode).await;
                let result = app
                    .invoke::<endpoint::EndpointHandle>(
                        "caller",
                        endpoint::HANDLE_OPERATION,
                        list_request("good"),
                    )
                    .await;
                match expected_status {
                    Some(status) => assert_eq!(result.unwrap().unwrap().status, status),
                    None => assert!(matches!(
                        result,
                        Err(RuntimeFailure::Unavailable {
                            capability: support::CAPABILITY_ID
                        })
                    )),
                }
                assert_eq!(
                    app.shutdown(StdDuration::from_secs(1)).await,
                    ShutdownOutcome::Clean
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn removing_agent_workspace_does_not_remove_support_provider() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
            let issuer = ActorAssertionIssuer::new("test.auth", b"support-web-test-key");
            let app = Kernel::start_native(
                domain_only_plan(),
                TokioDriver::new(),
                NativePluginRegistry::new()
                    .with_factory(EmptyFactory)
                    .with_factory(DomainFactory {
                        verifier: issuer.verifier(),
                        now,
                        observed_actor: Rc::new(Cell::new(false)),
                        mode: SupportMode::Success,
                        require_actor: false,
                    }),
            )
            .await
            .unwrap();

            let response = app
                .invoke::<support::SupportCaseListCases>(
                    "caller",
                    support::LIST_CASES_OPERATION,
                    list_cases_request(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response.cases[0].identifier, "SUP-1");
            assert_eq!(
                app.shutdown(StdDuration::from_secs(1)).await,
                ShutdownOutcome::Clean
            );
        })
        .await;
}

async fn start_web_app(
    issuer: ActorAssertionIssuer,
    now: OffsetDateTime,
    observed_actor: Rc<Cell<bool>>,
    mode: SupportMode,
) -> lenso_kernel::NativeApp {
    Kernel::start_native(
        web_plan(),
        TokioDriver::new(),
        NativePluginRegistry::new()
            .with_linked_factories()
            .with_factory(EmptyFactory)
            .with_factory(TestAuthFactory {
                issuer: issuer.clone(),
                now,
            })
            .with_factory(DomainFactory {
                verifier: issuer.verifier(),
                now,
                observed_actor,
                mode,
                require_actor: true,
            }),
    )
    .await
    .unwrap()
}

#[derive(Clone, Copy, Debug)]
struct EmptyFactory;

impl NativePluginFactory for EmptyFactory {
    fn package_id(&self) -> &'static str {
        CALLER_PACKAGE
    }

    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::default())
    }
}

#[derive(Clone, Debug)]
struct TestAuthFactory {
    issuer: ActorAssertionIssuer,
    now: OffsetDateTime,
}

impl NativePluginFactory for TestAuthFactory {
    fn package_id(&self) -> &'static str {
        AUTH_PACKAGE
    }

    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::new(vec![Rc::new(AuthEndpoint::new(
            TestAuth {
                issuer: self.issuer.clone(),
                now: self.now,
            },
        ))]))
    }
}

#[derive(Clone, Debug)]
struct TestAuth {
    issuer: ActorAssertionIssuer,
    now: OffsetDateTime,
}

impl AuthProvider for TestAuth {
    fn authenticate(
        &self,
        _context: InvocationContext,
        request: auth::AuthenticateRequest,
    ) -> NativeRequestFuture<Auth> {
        let result = match request.credential {
            Some(credential)
                if credential.scheme == "bearer"
                    && matches!(credential.value.as_str(), "good" | "wrong-kind") =>
            {
                let actor_kind = if credential.value == "good" {
                    "user"
                } else {
                    "service"
                };
                let assertion = self.issuer.issue(
                    "user_1",
                    actor_kind,
                    "test",
                    SUPPORT_OPERATIONS
                        .iter()
                        .map(|operation| audience(support::CAPABILITY_ID, operation)),
                    Validity::new(
                        self.now - Duration::seconds(1),
                        self.now + Duration::minutes(1),
                    )
                    .unwrap(),
                    BTreeMap::new(),
                );
                Ok(Ok(authenticated_response(&assertion)))
            }
            _ => Ok(Err(auth::AuthenticateError::Invalid)),
        };
        Box::pin(std::future::ready(result))
    }
}

#[derive(Clone, Debug)]
struct DomainFactory {
    verifier: lenso_auth_sdk::ActorAssertionVerifier,
    now: OffsetDateTime,
    observed_actor: Rc<Cell<bool>>,
    mode: SupportMode,
    require_actor: bool,
}

impl NativePluginFactory for DomainFactory {
    fn package_id(&self) -> &'static str {
        DOMAIN_PACKAGE
    }

    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::new(vec![Rc::new(
            FakeSupportEndpoint {
                verifier: self.verifier.clone(),
                now: self.now,
                observed_actor: Rc::clone(&self.observed_actor),
                mode: self.mode,
                require_actor: self.require_actor,
            },
        )]))
    }
}

#[derive(Debug)]
struct FakeSupportEndpoint {
    verifier: lenso_auth_sdk::ActorAssertionVerifier,
    now: OffsetDateTime,
    observed_actor: Rc<Cell<bool>>,
    mode: SupportMode,
    require_actor: bool,
}

impl NativeRequestEndpoint for FakeSupportEndpoint {
    fn capability_id(&self) -> &'static str {
        support::CAPABILITY_ID
    }

    fn descriptor_version(&self) -> &'static str {
        support::DESCRIPTOR_VERSION
    }

    fn operations(&self) -> &'static [&'static str] {
        SUPPORT_OPERATIONS
    }

    fn invoke(
        &self,
        operation: &str,
        request: Box<dyn Any>,
        context: InvocationContext,
    ) -> LocalBoxFuture<'static, Result<Result<Box<dyn Any>, Box<dyn Any>>, RuntimeFailure>> {
        if self.require_actor {
            if self
                .verifier
                .project_context::<SupportActor>(
                    &context,
                    support::CAPABILITY_ID,
                    operation,
                    &FixedClock::new(self.now),
                )
                .is_err()
            {
                return ready_domain_error(support::ListCasesError::Unauthenticated);
            }
            self.observed_actor.set(true);
        }

        if operation == support::LIST_CASES_OPERATION {
            if request.downcast::<support::ListCasesRequest>().is_err() {
                return protocol_violation();
            }
            return match self.mode {
                SupportMode::Success => ready_success(list_cases_response()),
                SupportMode::Forbidden => ready_domain_error(support::ListCasesError::Forbidden),
                SupportMode::Runtime => {
                    Box::pin(std::future::ready(Err(RuntimeFailure::Unavailable {
                        capability: support::CAPABILITY_ID,
                    })))
                }
            };
        }

        if operation == support::GET_CASE_OPERATION {
            let Ok(request) = request.downcast::<support::GetCaseRequest>() else {
                return protocol_violation();
            };
            if request.organization_id != "org_1" || request.case_ref != "case_1" {
                return ready_domain_error(support::GetCaseError::InvalidRequest);
            }
            return ready_success(get_case_response());
        }

        if operation == support::ADD_MESSAGE_OPERATION {
            let Ok(request) = request.downcast::<support::AddMessageRequest>() else {
                return protocol_violation();
            };
            if request.organization_id != "org_1"
                || request.case_id != "case_1"
                || request.body != "We are investigating."
                || !matches!(
                    request.visibility,
                    support::AddMessageRequestVisibility::Public
                )
            {
                return ready_domain_error(support::AddMessageError::InvalidRequest);
            }
            if request.expected_revision != "rev-1" {
                return ready_domain_error(support::AddMessageError::RevisionConflict);
            }
            return ready_success(add_message_response());
        }

        if operation == support::TRANSITION_CASE_OPERATION {
            let Ok(request) = request.downcast::<support::TransitionCaseRequest>() else {
                return protocol_violation();
            };
            if request.expected_revision != "rev-2" {
                return ready_domain_error(support::TransitionCaseError::RevisionConflict);
            }
            if request.organization_id != "org_1"
                || request.case_id != "case_1"
                || !matches!(
                    request.state,
                    support::TransitionCaseRequestState::WaitingCustomer
                )
            {
                return ready_domain_error(support::TransitionCaseError::InvalidRequest);
            }
            return ready_success(transition_case_response());
        }

        Box::pin(std::future::ready(Err(RuntimeFailure::UnknownOperation {
            capability: support::CAPABILITY_ID,
            operation: operation.to_owned(),
        })))
    }
}

fn ready_success<T: Any>(response: T) -> EndpointFuture {
    let response: Box<dyn Any> = Box::new(response);
    Box::pin(std::future::ready(Ok(Ok(response))))
}

fn ready_domain_error<E: Any>(error: E) -> EndpointFuture {
    let error: Box<dyn Any> = Box::new(error);
    Box::pin(std::future::ready(Ok(Err(error))))
}

fn protocol_violation() -> EndpointFuture {
    Box::pin(std::future::ready(Err(RuntimeFailure::ProtocolViolation {
        capability: support::CAPABILITY_ID,
    })))
}

#[derive(Debug)]
struct SupportActor;

impl TypedActor for SupportActor {
    fn from_assertion(assertion: &ActorAssertion) -> Result<Self, ActorProjectionError> {
        if assertion.actor_kind() != "user" || assertion.subject() != "user_1" {
            return Err(ActorProjectionError::UnexpectedActorKind {
                expected: "user".to_owned(),
                actual: assertion.actor_kind().to_owned(),
            });
        }
        Ok(Self)
    }
}

fn web_plan() -> ResolvedAppPlan {
    let caller = PluginInstancePlan::new("caller", CALLER_PACKAGE).with_requirement(
        CapabilityRequirementPlan::one(endpoint::CAPABILITY_ID, endpoint::DESCRIPTOR_VERSION),
    );
    let web = PluginInstancePlan::new("support-web", PACKAGE_ID)
        .with_capability(CapabilityEndpointPlan::new(
            endpoint::CAPABILITY_ID,
            endpoint::DESCRIPTOR_VERSION,
            [endpoint::DESCRIBE_OPERATION, endpoint::HANDLE_OPERATION],
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            auth::CAPABILITY_ID,
            auth::DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            support::CAPABILITY_ID,
            support::DESCRIPTOR_VERSION,
        ));
    let auth_provider =
        PluginInstancePlan::new("auth", AUTH_PACKAGE).with_capability(CapabilityEndpointPlan::new(
            auth::CAPABILITY_ID,
            auth::DESCRIPTOR_VERSION,
            [auth::AUTHENTICATE_OPERATION],
        ));
    AppComposition::new(
        vec![caller, web, auth_provider, domain_instance()],
        vec![
            CapabilityBinding::new(
                "caller",
                endpoint::CAPABILITY_ID,
                endpoint::DESCRIPTOR_VERSION,
                "support-web",
            ),
            CapabilityBinding::new(
                "support-web",
                auth::CAPABILITY_ID,
                auth::DESCRIPTOR_VERSION,
                "auth",
            ),
            CapabilityBinding::new(
                "support-web",
                support::CAPABILITY_ID,
                support::DESCRIPTOR_VERSION,
                "domain",
            ),
        ],
    )
    .resolve()
    .unwrap()
}

fn domain_only_plan() -> ResolvedAppPlan {
    let caller = PluginInstancePlan::new("caller", CALLER_PACKAGE).with_requirement(
        CapabilityRequirementPlan::one(support::CAPABILITY_ID, support::DESCRIPTOR_VERSION),
    );
    AppComposition::new(
        vec![caller, domain_instance()],
        vec![CapabilityBinding::new(
            "caller",
            support::CAPABILITY_ID,
            support::DESCRIPTOR_VERSION,
            "domain",
        )],
    )
    .resolve()
    .unwrap()
}

fn domain_instance() -> PluginInstancePlan {
    PluginInstancePlan::new("domain", DOMAIN_PACKAGE).with_capability(CapabilityEndpointPlan::new(
        support::CAPABILITY_ID,
        support::DESCRIPTOR_VERSION,
        SUPPORT_OPERATIONS.iter().copied(),
    ))
}

fn list_cases_request() -> support::ListCasesRequest {
    support::ListCasesRequest {
        assignee_subject: None,
        cursor: None,
        limit: 10,
        organization_id: "org_1".to_owned(),
        requester_subject: None,
        state: None,
    }
}

fn list_request(token: &str) -> HandleRequest {
    handle_request(
        "support.web.cases.list",
        "GET",
        "/api/support/cases",
        Vec::new(),
        Some("organization_id=org_1&limit=10"),
        None,
        token,
    )
}

fn detail_request(token: &str) -> HandleRequest {
    handle_request(
        "support.web.cases.detail",
        "GET",
        "/api/support/cases/case_1",
        vec![("case_ref", "case_1")],
        Some("organization_id=org_1"),
        None,
        token,
    )
}

fn public_reply_request(token: &str, expected_revision: &str) -> HandleRequest {
    handle_request(
        "support.web.messages.add-public",
        "POST",
        "/api/support/cases/case_1/messages",
        vec![("case_id", "case_1")],
        None,
        Some(json!({
            "body": "We are investigating.",
            "case_id": "case_1",
            "expected_revision": expected_revision,
            "idempotency_key": "reply-1",
            "organization_id": "org_1",
            "visibility": "public"
        })),
        token,
    )
}

fn transition_request(token: &str, expected_revision: &str) -> HandleRequest {
    handle_request(
        "support.web.cases.transition",
        "POST",
        "/api/support/cases/case_1/transition",
        vec![("case_id", "case_1")],
        None,
        Some(json!({
            "case_id": "case_1",
            "expected_revision": expected_revision,
            "idempotency_key": format!("transition-{expected_revision}"),
            "organization_id": "org_1",
            "reason": "Waiting for confirmation",
            "state": "waiting_customer"
        })),
        token,
    )
}

fn handle_request(
    route_id: &str,
    method: &str,
    path: &str,
    path_parameters: Vec<(&str, &str)>,
    query: Option<&str>,
    body: Option<Value>,
    token: &str,
) -> HandleRequest {
    let has_body = body.is_some();
    HandleRequest {
        body: body
            .map_or_else(Vec::new, |value| serde_json::to_vec(&value).unwrap())
            .into(),
        credential: Some(HandleRequestCredential {
            scheme: "bearer".to_owned(),
            value: token.to_owned(),
        }),
        headers: if has_body {
            vec![HandleRequestHeadersItem {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            }]
        } else {
            Vec::new()
        },
        method: method.to_owned(),
        path: path.to_owned(),
        path_parameters: path_parameters
            .into_iter()
            .map(|(name, value)| HandleRequestPathParametersItem {
                name: name.to_owned(),
                value: value.to_owned(),
            })
            .collect(),
        query: query.map(str::to_owned),
        request_id: format!("test-{route_id}"),
        route_id: route_id.to_owned(),
    }
}

fn list_cases_response() -> support::ListCasesResponse {
    fixture(json!({
        "cases": [case_json("rev-1", "open")],
        "next_cursor": null
    }))
}

fn get_case_response() -> support::GetCaseResponse {
    fixture(case_json("rev-1", "open"))
}

fn add_message_response() -> support::AddMessageResponse {
    fixture(json!({
        "author_subject": "user_1",
        "body": "We are investigating.",
        "case_id": "case_1",
        "case_revision": "rev-2",
        "created_at": NOW,
        "message_id": "message_1",
        "visibility": "public"
    }))
}

fn transition_case_response() -> support::TransitionCaseResponse {
    fixture(case_json("rev-3", "waiting_customer"))
}

fn case_json(revision: &str, state: &str) -> Value {
    json!({
        "assignee_subject": "agent_1",
        "case_id": "case_1",
        "closed_at": null,
        "created_at": NOW,
        "creator_subject": "requester_1",
        "description": "Checkout fails after payment.",
        "identifier": "SUP-1",
        "organization_id": "org_1",
        "priority": "high",
        "requester_subject": "requester_1",
        "resolved_at": null,
        "revision": revision,
        "state": state,
        "title": "Checkout failure",
        "updated_at": NOW
    })
}

fn fixture<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn body_json(response: &endpoint::HandleResponse) -> Value {
    serde_json::from_slice(&response.body).unwrap()
}

fn body_text(response: &endpoint::HandleResponse) -> String {
    String::from_utf8_lossy(&response.body).into_owned()
}
