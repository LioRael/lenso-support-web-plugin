//! Standalone linked Web surface for the Lenso Support Case capability.

mod assets;

use std::fmt::Debug;

use lenso::prelude::*;
use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_auth as auth;
use lenso_capability_http_endpoint::{
    self as http_endpoint_contract, EndpointHandleInvocationError, ExtractorFuture,
    ExtractorRejection, FromRequest, HandleRequest, HandleResponse, HandleResponseHeadersItem,
    Json, Path, QueryParams, endpoint,
    response::{self, HeaderValue, StatusCode, header},
};
use lenso_capability_support_case as support;
use lenso_kernel::{InvocationContext, RuntimeFailure};
use serde::{Deserialize, Serialize};

/// Forces this native Plugin crate to be retained by a linked Host.
pub const fn link() {}

#[lenso::plugin]
#[derive(Clone, Debug, Default)]
pub struct SupportWebPlugin {
    auth: Port<auth::AuthClient>,
    support: Port<support::SupportCaseClient>,
}

#[endpoint]
impl SupportWebPlugin {
    #[get("support.web.page", "/support")]
    async fn page(&self) -> Result<HandleResponse, EndpointHandleInvocationError> {
        std::future::ready(()).await;
        Ok(asset(
            StatusCode::OK,
            "text/html; charset=utf-8",
            assets::PAGE,
        ))
    }

    #[get("support.web.css", "/support/assets/app.css")]
    async fn css(&self) -> Result<HandleResponse, EndpointHandleInvocationError> {
        std::future::ready(()).await;
        Ok(asset(
            StatusCode::OK,
            "text/css; charset=utf-8",
            assets::CSS,
        ))
    }

    #[get("support.web.js", "/support/assets/app.js")]
    async fn javascript(&self) -> Result<HandleResponse, EndpointHandleInvocationError> {
        std::future::ready(()).await;
        Ok(asset(
            StatusCode::OK,
            "text/javascript; charset=utf-8",
            assets::JS,
        ))
    }

    #[get("support.web.cases.list", "/api/support/cases")]
    async fn list_cases(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        QueryParams(query): QueryParams<ListCasesQuery>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        json_result(
            self.support
                .list_cases_with_context(context, query.into_request())
                .await,
            StatusCode::OK,
        )
    }

    #[post("support.web.cases.create", "/api/support/cases")]
    async fn create_case(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Json(request): Json<support::CreateCaseRequest>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        json_result(
            self.support
                .create_case_with_context(context, request)
                .await,
            StatusCode::CREATED,
        )
    }

    #[get("support.web.cases.detail", "/api/support/cases/{case_ref}")]
    async fn get_case(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Path(path): Path<CaseRefPath>,
        QueryParams(query): QueryParams<OrganizationQuery>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        json_result(
            self.support
                .get_case_with_context(
                    context,
                    support::GetCaseRequest {
                        case_ref: path.case_ref,
                        organization_id: query.organization_id,
                    },
                )
                .await,
            StatusCode::OK,
        )
    }

    #[patch("support.web.cases.update", "/api/support/cases/{case_id}")]
    async fn update_case(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Path(path): Path<CasePath>,
        Json(request): Json<support::UpdateCaseRequest>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        if path.case_id != request.case_id {
            return Ok(path_mismatch());
        }
        json_result(
            self.support
                .update_case_with_context(context, request)
                .await,
            StatusCode::OK,
        )
    }

    #[post("support.web.cases.assign", "/api/support/cases/{case_id}/assign")]
    async fn assign_case(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Path(path): Path<CasePath>,
        Json(request): Json<support::AssignCaseRequest>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        if path.case_id != request.case_id {
            return Ok(path_mismatch());
        }
        json_result(
            self.support
                .assign_case_with_context(context, request)
                .await,
            StatusCode::OK,
        )
    }

    #[post(
        "support.web.cases.transition",
        "/api/support/cases/{case_id}/transition"
    )]
    async fn transition_case(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Path(path): Path<CasePath>,
        Json(request): Json<support::TransitionCaseRequest>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        if path.case_id != request.case_id {
            return Ok(path_mismatch());
        }
        json_result(
            self.support
                .transition_case_with_context(context, request)
                .await,
            StatusCode::OK,
        )
    }

    #[get("support.web.messages.list", "/api/support/cases/{case_id}/messages")]
    async fn list_messages(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Path(path): Path<CasePath>,
        QueryParams(query): QueryParams<MessagePageQuery>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        json_result(
            self.support
                .list_messages_with_context(
                    context,
                    support::ListMessagesRequest {
                        case_id: path.case_id,
                        cursor: query.cursor,
                        limit: query.limit,
                        organization_id: query.organization_id,
                    },
                )
                .await,
            StatusCode::OK,
        )
    }

    #[post(
        "support.web.messages.add-public",
        "/api/support/cases/{case_id}/messages"
    )]
    async fn add_public_message(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Path(path): Path<CasePath>,
        Json(request): Json<support::AddMessageRequest>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        if path.case_id != request.case_id {
            return Ok(path_mismatch());
        }
        if !matches!(
            &request.visibility,
            support::AddMessageRequestVisibility::Public
        ) {
            return Ok(visibility_mismatch("public"));
        }
        json_result(
            self.support
                .add_message_with_context(context, request)
                .await,
            StatusCode::CREATED,
        )
    }

    #[post(
        "support.web.messages.add-internal",
        "/api/support/cases/{case_id}/notes"
    )]
    async fn add_internal_note(
        &self,
        _actor: AuthenticatedUser,
        context: InvocationContext,
        Path(path): Path<CasePath>,
        Json(request): Json<support::AddMessageRequest>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        if path.case_id != request.case_id {
            return Ok(path_mismatch());
        }
        if !matches!(
            &request.visibility,
            support::AddMessageRequestVisibility::Internal
        ) {
            return Ok(visibility_mismatch("internal"));
        }
        json_result(
            self.support
                .add_message_with_context(context, request)
                .await,
            StatusCode::CREATED,
        )
    }
}

#[derive(Debug)]
struct AuthenticatedUser;

impl FromRequest<SupportWebPlugin> for AuthenticatedUser {
    fn from_request<'a>(
        provider: &'a SupportWebPlugin,
        context: &'a mut InvocationContext,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            let evidence = request
                .credential
                .as_ref()
                .map(|credential| CredentialEvidence::new(&credential.scheme, &credential.value));
            let response = provider
                .auth
                .authenticate_with_context(context.clone(), authenticate_request(evidence))
                .await
                .map_err(|error| -> ExtractorRejection {
                    match error {
                        auth::AuthInvocationError::Domain(_) => authentication_problem().into(),
                        auth::AuthInvocationError::Runtime(error) => {
                            EndpointHandleInvocationError::Runtime(error).into()
                        }
                    }
                })?;
            let outcome = decode_auth_response(response).map_err(|_| {
                EndpointHandleInvocationError::Runtime(RuntimeFailure::ProtocolViolation {
                    capability: auth::CAPABILITY_ID,
                })
            })?;
            let AuthOutcome::Authenticated(assertion) = outcome else {
                return Err(authentication_problem().into());
            };
            if assertion.actor_kind() != "user" {
                return Err(response::problem(
                    StatusCode::FORBIDDEN,
                    "unsupported_actor",
                    "This Web surface requires an authenticated user actor.",
                )
                .into());
            }
            *context = assertion.attach(context.clone()).map_err(|error| {
                EndpointHandleInvocationError::Runtime(RuntimeFailure::Internal {
                    detail: format!("could not attach authenticated actor assertion: {error}"),
                })
            })?;
            Ok(Self)
        })
    }
}

fn authentication_problem() -> HandleResponse {
    response::problem(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "Provide a valid Bearer credential.",
    )
    .with_header(
        &header::WWW_AUTHENTICATE,
        &HeaderValue::from_static("Bearer"),
    )
    .expect("the static WWW-Authenticate header is valid")
}

fn asset(status: StatusCode, content_type: &str, body: &str) -> HandleResponse {
    HandleResponse {
        body: body.as_bytes().to_vec().into(),
        headers: vec![HandleResponseHeadersItem {
            name: "content-type".to_owned(),
            value: content_type.to_owned(),
        }],
        status: i64::from(status.as_u16()),
    }
}

fn path_mismatch() -> HandleResponse {
    response::problem(
        StatusCode::BAD_REQUEST,
        "path_body_mismatch",
        "The path and JSON body must name the same case_id.",
    )
}

fn visibility_mismatch(expected: &str) -> HandleResponse {
    response::problem(
        StatusCode::BAD_REQUEST,
        "message_visibility_mismatch",
        format!("This route accepts only {expected} messages."),
    )
}

trait IntoWebError {
    fn into_web_error(self) -> Result<HandleResponse, EndpointHandleInvocationError>;
}

fn json_result<T, E>(
    result: Result<T, E>,
    status: StatusCode,
) -> Result<HandleResponse, EndpointHandleInvocationError>
where
    T: Serialize,
    E: IntoWebError,
{
    match result {
        Ok(value) => response::json(status, &value).map_err(Into::into),
        Err(error) => error.into_web_error(),
    }
}

fn domain_problem(error: &impl Debug) -> Result<HandleResponse, EndpointHandleInvocationError> {
    let variant = format!("{error:?}");
    if variant.starts_with("Unknown(") {
        return Err(EndpointHandleInvocationError::Runtime(
            RuntimeFailure::ProtocolViolation {
                capability: support::CAPABILITY_ID,
            },
        ));
    }
    let code = snake_case(&variant);
    let status = match variant.as_str() {
        "Unauthenticated" => StatusCode::UNAUTHORIZED,
        // The target owns requester/agent visibility. Do not reveal whether a
        // forbidden case exists by distinguishing it from CaseNotFound.
        "Forbidden" | "CaseNotFound" => StatusCode::NOT_FOUND,
        "IdempotencyConflict" | "RevisionConflict" | "InvalidTransition" => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    };
    let mut problem = response::problem(
        status,
        code.clone(),
        format!("The Support Case capability rejected this operation ({code})."),
    );
    if status == StatusCode::UNAUTHORIZED {
        problem = problem.with_header(
            &header::WWW_AUTHENTICATE,
            &HeaderValue::from_static("Bearer"),
        )?;
    }
    Ok(problem)
}

fn snake_case(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 4);
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() && index != 0 {
            output.push('_');
        }
        output.push(character.to_ascii_lowercase());
    }
    output
}

macro_rules! impl_web_error {
    ($type:path) => {
        impl IntoWebError for $type {
            fn into_web_error(self) -> Result<HandleResponse, EndpointHandleInvocationError> {
                match self {
                    Self::Domain(error) => domain_problem(&error),
                    Self::Runtime(error) => Err(EndpointHandleInvocationError::Runtime(error)),
                }
            }
        }
    };
}

impl_web_error!(support::SupportCaseAddMessageInvocationError);
impl_web_error!(support::SupportCaseAssignCaseInvocationError);
impl_web_error!(support::SupportCaseCreateCaseInvocationError);
impl_web_error!(support::SupportCaseGetCaseInvocationError);
impl_web_error!(support::SupportCaseListCasesInvocationError);
impl_web_error!(support::SupportCaseListMessagesInvocationError);
impl_web_error!(support::SupportCaseTransitionCaseInvocationError);
impl_web_error!(support::SupportCaseUpdateCaseInvocationError);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CasePath {
    case_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseRefPath {
    case_ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrganizationQuery {
    organization_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListCasesQuery {
    organization_id: String,
    #[serde(default)]
    requester_subject: Option<String>,
    #[serde(default)]
    assignee_subject: Option<String>,
    #[serde(default)]
    state: Option<support::ListCasesRequestState>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}

impl ListCasesQuery {
    fn into_request(self) -> support::ListCasesRequest {
        support::ListCasesRequest {
            assignee_subject: self.assignee_subject,
            cursor: self.cursor,
            limit: self.limit,
            organization_id: self.organization_id,
            requester_subject: self.requester_subject,
            state: self.state,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagePageQuery {
    organization_id: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}

const fn default_limit() -> i64 {
    40
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use lenso_capability_http_endpoint::testing::EndpointTest;

    use super::*;

    #[test]
    fn serves_self_contained_page_and_assets_without_connected_ports() {
        block_on(async {
            let endpoint = EndpointTest::new(SupportWebPlugin::default());
            let page = endpoint.request("support.web.page").send().await.unwrap();
            assert_eq!(page.status(), StatusCode::OK);
            assert_eq!(
                page.header("content-type"),
                Some("text/html; charset=utf-8")
            );
            assert!(page.into_inner().body.starts_with(b"<!doctype html>"));

            let css = endpoint.request("support.web.css").send().await.unwrap();
            assert_eq!(css.header("content-type"), Some("text/css; charset=utf-8"));

            let javascript = endpoint.request("support.web.js").send().await.unwrap();
            assert_eq!(
                javascript.header("content-type"),
                Some("text/javascript; charset=utf-8")
            );
            assert!(
                javascript
                    .into_inner()
                    .body
                    .windows(12)
                    .any(|chunk| chunk == b"/api/support")
            );
        });
    }

    #[test]
    fn descriptor_declares_exact_provided_and_required_capabilities() {
        let descriptor: serde_json::Value = serde_json::from_str(PLUGIN_DESCRIPTOR_JSON).unwrap();
        let provided = descriptor["provided_capabilities"].as_array().unwrap();
        assert_eq!(provided.len(), 1);
        assert_eq!(provided[0]["capability_id"], "lenso.http.endpoint@1");
        assert_eq!(provided[0]["descriptor_version"], "1.1.0");

        let mut required = descriptor["required_capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry["capability_id"].as_str().unwrap(),
                    entry["descriptor_version"].as_str().unwrap(),
                    entry["cardinality"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        required.sort_unstable();
        assert_eq!(
            required,
            vec![
                ("lenso.auth@1", "1.0.0", "one"),
                ("lenso.support-case@1", "1.0.0", "one"),
            ]
        );
    }

    #[test]
    fn keeps_forbidden_cases_visibility_safe_and_conflicts_explicit() {
        assert_eq!(
            domain_problem(&support::GetCaseError::Forbidden)
                .unwrap()
                .status,
            404
        );
        assert_eq!(
            domain_problem(&support::TransitionCaseError::RevisionConflict)
                .unwrap()
                .status,
            409
        );
        assert_eq!(
            domain_problem(&support::CreateCaseError::InvalidRequest)
                .unwrap()
                .status,
            400
        );
    }
}
