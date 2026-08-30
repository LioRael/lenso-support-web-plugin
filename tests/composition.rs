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
use lenso_capability_http_endpoint::{HandleRequest, HandleRequestCredential};
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
use time::{Duration, OffsetDateTime};

const CALLER_PACKAGE: &str = "test.support-web-caller";
const AUTH_PACKAGE: &str = "test.support-web-auth";
const DOMAIN_PACKAGE: &str = "test.support-web-domain";

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
async fn authenticates_forwards_actor_and_preserves_visibility_and_runtime_boundaries() {
    tokio::task::LocalSet::new()
        .run_until(async {
            lenso_support_web_plugin::link();
            let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
            let issuer = ActorAssertionIssuer::new("test.auth", b"support-web-test-key");

            for (mode, expected_status) in [
                (SupportMode::Success, Some(200)),
                (SupportMode::Forbidden, Some(404)),
                (SupportMode::Runtime, None),
            ] {
                let observed_actor = Rc::new(Cell::new(false));
                let app = Kernel::start_native(
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
                            observed_actor: Rc::clone(&observed_actor),
                            mode,
                            require_actor: true,
                        }),
                )
                .await
                .unwrap();

                if matches!(mode, SupportMode::Success) {
                    let unauthenticated = app
                        .invoke::<endpoint::EndpointHandle>(
                            "caller",
                            endpoint::HANDLE_OPERATION,
                            list_request("bad"),
                        )
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(unauthenticated.status, 401);

                    let wrong_actor = app
                        .invoke::<endpoint::EndpointHandle>(
                            "caller",
                            endpoint::HANDLE_OPERATION,
                            list_request("wrong-kind"),
                        )
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(wrong_actor.status, 403);
                }

                let result = app
                    .invoke::<endpoint::EndpointHandle>(
                        "caller",
                        endpoint::HANDLE_OPERATION,
                        list_request("good"),
                    )
                    .await;
                match expected_status {
                    Some(status) => {
                        let response = result.unwrap().unwrap();
                        assert_eq!(response.status, status);
                        assert!(observed_actor.get());
                    }
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
async fn removing_web_instance_does_not_remove_support_provider() {
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
                    support::ListCasesRequest {
                        assignee_subject: None,
                        cursor: None,
                        limit: 10,
                        organization_id: "org_1".to_owned(),
                        requester_subject: None,
                        state: None,
                    },
                )
                .await
                .unwrap()
                .unwrap();
            assert!(response.cases.is_empty());
            assert_eq!(
                app.shutdown(StdDuration::from_secs(1)).await,
                ShutdownOutcome::Clean
            );
        })
        .await;
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
                    [audience(
                        support::CAPABILITY_ID,
                        support::LIST_CASES_OPERATION,
                    )],
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
        if operation != support::LIST_CASES_OPERATION {
            return Box::pin(std::future::ready(Err(RuntimeFailure::UnknownOperation {
                capability: support::CAPABILITY_ID,
                operation: operation.to_owned(),
            })));
        }
        if request.downcast::<support::ListCasesRequest>().is_err() {
            return Box::pin(std::future::ready(Err(RuntimeFailure::ProtocolViolation {
                capability: support::CAPABILITY_ID,
            })));
        }
        if self.require_actor {
            if self
                .verifier
                .project_context::<SupportActor>(
                    &context,
                    support::CAPABILITY_ID,
                    support::LIST_CASES_OPERATION,
                    &FixedClock::new(self.now),
                )
                .is_err()
            {
                return Box::pin(std::future::ready(Ok(Err(Box::new(
                    support::ListCasesError::Unauthenticated,
                )
                    as Box<dyn Any>))));
            }
            self.observed_actor.set(true);
        }
        let result = match self.mode {
            SupportMode::Success => Ok(Ok(Box::new(support::ListCasesResponse {
                cases: Vec::new(),
                next_cursor: None,
            }) as Box<dyn Any>)),
            SupportMode::Forbidden => Ok(Err(
                Box::new(support::ListCasesError::Forbidden) as Box<dyn Any>
            )),
            SupportMode::Runtime => Err(RuntimeFailure::Unavailable {
                capability: support::CAPABILITY_ID,
            }),
        };
        Box::pin(std::future::ready(result))
    }
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

fn list_request(token: &str) -> HandleRequest {
    HandleRequest {
        body: Vec::new().into(),
        credential: Some(HandleRequestCredential {
            scheme: "bearer".to_owned(),
            value: token.to_owned(),
        }),
        headers: Vec::new(),
        method: "GET".to_owned(),
        path: "/api/support/cases".to_owned(),
        path_parameters: Vec::new(),
        query: Some("organization_id=org_1&limit=10".to_owned()),
        request_id: "request-1".to_owned(),
        route_id: "support.web.cases.list".to_owned(),
    }
}
