pub use analytics::*;

pub mod routes {
    use actix_web::{web, Responder, Scope};
    use analytics::{
        api_event::api_events_core, connector_events::connector_events_core,
        errors::AnalyticsError, lambda_utils::invoke_lambda, opensearch::OpenSearchError,
        outgoing_webhook_event::outgoing_webhook_events_core, sdk_events::sdk_events_core,
        AnalyticsFlow,
    };
    use api_models::analytics::{
        search::{
            GetGlobalSearchRequest, GetSearchRequest, GetSearchRequestWithIndex, SearchIndex,
        },
        GenerateReportRequest, GetActivePaymentsMetricRequest, GetApiEventFiltersRequest,
        GetApiEventMetricRequest, GetAuthEventMetricRequest, GetDisputeMetricRequest,
        GetFrmFilterRequest, GetFrmMetricRequest, GetPaymentFiltersRequest,
        GetPaymentIntentFiltersRequest, GetPaymentIntentMetricRequest, GetPaymentMetricRequest,
        GetRefundFilterRequest, GetRefundMetricRequest, GetSdkEventFiltersRequest,
        GetSdkEventMetricRequest, ReportRequest,
    };
    use error_stack::ResultExt;

    use crate::{
        consts::opensearch::OPENSEARCH_INDEX_PERMISSIONS,
        core::{api_locking, errors::user::UserErrors},
        db::user::UserInterface,
        routes::AppState,
        services::{
            api,
            authentication::{self as auth, AuthenticationData, UserFromToken},
            authorization::{permissions::Permission, roles::RoleInfo},
            ApplicationResponse,
        },
        types::domain::UserEmail,
    };

    pub struct Analytics;

    impl Analytics {
        pub fn server(state: AppState) -> Scope {
            web::scope("/analytics")
                .app_data(web::Data::new(state))
                .service(
                    web::scope("/v1")
                        .service(
                            web::resource("metrics/payments")
                                .route(web::post().to(get_payment_metrics)),
                        )
                        .service(
                            web::resource("metrics/refunds")
                                .route(web::post().to(get_refunds_metrics)),
                        )
                        .service(
                            web::resource("filters/payments")
                                .route(web::post().to(get_payment_filters)),
                        )
                        .service(
                            web::resource("filters/frm").route(web::post().to(get_frm_filters)),
                        )
                        .service(
                            web::resource("filters/refunds")
                                .route(web::post().to(get_refund_filters)),
                        )
                        .service(web::resource("{domain}/info").route(web::get().to(get_info)))
                        .service(
                            web::resource("report/dispute")
                                .route(web::post().to(generate_dispute_report)),
                        )
                        .service(
                            web::resource("report/refunds")
                                .route(web::post().to(generate_refund_report)),
                        )
                        .service(
                            web::resource("report/payments")
                                .route(web::post().to(generate_payment_report)),
                        )
                        .service(
                            web::resource("metrics/sdk_events")
                                .route(web::post().to(get_sdk_event_metrics)),
                        )
                        .service(
                            web::resource("metrics/active_payments")
                                .route(web::post().to(get_active_payments_metrics)),
                        )
                        .service(
                            web::resource("filters/sdk_events")
                                .route(web::post().to(get_sdk_event_filters)),
                        )
                        .service(
                            web::resource("metrics/auth_events")
                                .route(web::post().to(get_auth_event_metrics)),
                        )
                        .service(
                            web::resource("metrics/frm").route(web::post().to(get_frm_metrics)),
                        )
                        .service(
                            web::resource("api_event_logs").route(web::get().to(get_api_events)),
                        )
                        .service(
                            web::resource("sdk_event_logs").route(web::post().to(get_sdk_events)),
                        )
                        .service(
                            web::resource("connector_event_logs")
                                .route(web::get().to(get_connector_events)),
                        )
                        .service(
                            web::resource("outgoing_webhook_event_logs")
                                .route(web::get().to(get_outgoing_webhook_events)),
                        )
                        .service(
                            web::resource("filters/api_events")
                                .route(web::post().to(get_api_event_filters)),
                        )
                        .service(
                            web::resource("metrics/api_events")
                                .route(web::post().to(get_api_events_metrics)),
                        )
                        .service(
                            web::resource("search")
                                .route(web::post().to(get_global_search_results)),
                        )
                        .service(
                            web::resource("search/{domain}")
                                .route(web::post().to(get_search_results)),
                        )
                        .service(
                            web::resource("filters/disputes")
                                .route(web::post().to(get_dispute_filters)),
                        )
                        .service(
                            web::resource("metrics/disputes")
                                .route(web::post().to(get_dispute_metrics)),
                        ),
                )
                .service(
                    web::scope("/v2")
                        .service(
                            web::resource("/metrics/payments")
                                .route(web::post().to(get_payment_intents_metrics)),
                        )
                        .service(
                            web::resource("/filters/payments")
                                .route(web::post().to(get_payment_intents_filters)),
                        ),
                )
        }
    }

    pub async fn get_info(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        domain: web::Path<analytics::AnalyticsDomain>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetInfo;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            domain.into_inner(),
            |_, _: (), domain: analytics::AnalyticsDomain, _| async {
                analytics::core::get_domain_info(domain)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::NoAuth,
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetPaymentMetricRequest` element.
    pub async fn get_payment_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetPaymentMetricRequest; 1]>,
    ) -> impl Responder {
        // safety: This shouldn't panic owing to the data type
        #[allow(clippy::expect_used)]
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetPaymentMetricRequest");
        let flow = AnalyticsFlow::GetPaymentMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::payments::get_metrics(
                    &state.pool,
                    &auth.merchant_account.merchant_id,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetPaymentIntentMetricRequest` element.
    pub async fn get_payment_intents_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetPaymentIntentMetricRequest; 1]>,
    ) -> impl Responder {
        // safety: This shouldn't panic owing to the data type
        #[allow(clippy::expect_used)]
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetPaymentIntentMetricRequest");
        let flow = AnalyticsFlow::GetPaymentIntentMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::payment_intents::get_metrics(
                    &state.pool,
                    &auth.merchant_account.merchant_id,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetRefundMetricRequest` element.
    pub async fn get_refunds_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetRefundMetricRequest; 1]>,
    ) -> impl Responder {
        #[allow(clippy::expect_used)]
        // safety: This shouldn't panic owing to the data type
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetRefundMetricRequest");
        let flow = AnalyticsFlow::GetRefundsMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::refunds::get_metrics(
                    &state.pool,
                    &auth.merchant_account.merchant_id,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetFrmMetricRequest` element.
    pub async fn get_frm_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetFrmMetricRequest; 1]>,
    ) -> impl Responder {
        #[allow(clippy::expect_used)]
        // safety: This shouldn't panic owing to the data type
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetFrmMetricRequest");
        let flow = AnalyticsFlow::GetFrmMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::frm::get_metrics(&state.pool, &auth.merchant_account.merchant_id, req)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetSdkEventMetricRequest` element.
    pub async fn get_sdk_event_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetSdkEventMetricRequest; 1]>,
    ) -> impl Responder {
        // safety: This shouldn't panic owing to the data type
        #[allow(clippy::expect_used)]
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetSdkEventMetricRequest");
        let flow = AnalyticsFlow::GetSdkMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::sdk_events::get_metrics(
                    &state.pool,
                    &auth.merchant_account.publishable_key,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetActivePaymentsMetricRequest` element.
    pub async fn get_active_payments_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetActivePaymentsMetricRequest; 1]>,
    ) -> impl Responder {
        // safety: This shouldn't panic owing to the data type
        #[allow(clippy::expect_used)]
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetActivePaymentsMetricRequest");
        let flow = AnalyticsFlow::GetActivePaymentsMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::active_payments::get_metrics(
                    &state.pool,
                    &auth.merchant_account.publishable_key,
                    &auth.merchant_account.merchant_id,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetAuthEventMetricRequest` element.
    pub async fn get_auth_event_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetAuthEventMetricRequest; 1]>,
    ) -> impl Responder {
        // safety: This shouldn't panic owing to the data type
        #[allow(clippy::expect_used)]
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetAuthEventMetricRequest");
        let flow = AnalyticsFlow::GetAuthMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::auth_events::get_metrics(
                    &state.pool,
                    &auth.merchant_account.merchant_id,
                    &auth.merchant_account.publishable_key,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_payment_filters(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetPaymentFiltersRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetPaymentFilters;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                analytics::payments::get_filters(
                    &state.pool,
                    req,
                    &auth.merchant_account.merchant_id,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_payment_intents_filters(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetPaymentIntentFiltersRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetPaymentIntentFilters;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                analytics::payment_intents::get_filters(
                    &state.pool,
                    req,
                    &auth.merchant_account.merchant_id,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_refund_filters(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetRefundFilterRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetRefundFilters;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req: GetRefundFilterRequest, _| async move {
                analytics::refunds::get_filters(
                    &state.pool,
                    req,
                    &auth.merchant_account.merchant_id,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_frm_filters(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetFrmFilterRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetFrmFilters;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req: GetFrmFilterRequest, _| async move {
                analytics::frm::get_filters(&state.pool, req, &auth.merchant_account.merchant_id)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_sdk_event_filters(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetSdkEventFiltersRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetSdkEventFilters;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                analytics::sdk_events::get_filters(
                    &state.pool,
                    req,
                    &auth.merchant_account.publishable_key,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_api_events(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Query<api_models::analytics::api_event::ApiLogsRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetApiEvents;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                api_events_core(&state.pool, req, auth.merchant_account.merchant_id)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_outgoing_webhook_events(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Query<
            api_models::analytics::outgoing_webhook_event::OutgoingWebhookLogsRequest,
        >,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetOutgoingWebhookEvents;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                outgoing_webhook_events_core(&state.pool, req, auth.merchant_account.merchant_id)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_sdk_events(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<api_models::analytics::sdk_events::SdkEventsRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetSdkEvents;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                sdk_events_core(&state.pool, req, &auth.merchant_account.publishable_key)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn generate_refund_report(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<ReportRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GenerateRefundReport;
        Box::pin(api::server_wrap(
            flow,
            state.clone(),
            &req,
            json_payload.into_inner(),
            |state, (auth, user_id): auth::AuthenticationDataWithUserId, payload, _| async move {
                let user = UserInterface::find_user_by_id(&*state.global_store, &user_id)
                    .await
                    .change_context(AnalyticsError::UnknownError)?;

                let user_email = UserEmail::from_pii_email(user.email)
                    .change_context(AnalyticsError::UnknownError)?
                    .get_secret();

                let lambda_req = GenerateReportRequest {
                    request: payload,
                    merchant_id: auth.merchant_account.merchant_id.to_string(),
                    email: user_email,
                };

                let json_bytes =
                    serde_json::to_vec(&lambda_req).map_err(|_| AnalyticsError::UnknownError)?;
                invoke_lambda(
                    &state.conf.report_download_config.refund_function,
                    &state.conf.report_download_config.region,
                    &json_bytes,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn generate_dispute_report(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<ReportRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GenerateDisputeReport;
        Box::pin(api::server_wrap(
            flow,
            state.clone(),
            &req,
            json_payload.into_inner(),
            |state, (auth, user_id): auth::AuthenticationDataWithUserId, payload, _| async move {
                let user = UserInterface::find_user_by_id(&*state.global_store, &user_id)
                    .await
                    .change_context(AnalyticsError::UnknownError)?;

                let user_email = UserEmail::from_pii_email(user.email)
                    .change_context(AnalyticsError::UnknownError)?
                    .get_secret();

                let lambda_req = GenerateReportRequest {
                    request: payload,
                    merchant_id: auth.merchant_account.merchant_id.to_string(),
                    email: user_email,
                };

                let json_bytes =
                    serde_json::to_vec(&lambda_req).map_err(|_| AnalyticsError::UnknownError)?;
                invoke_lambda(
                    &state.conf.report_download_config.dispute_function,
                    &state.conf.report_download_config.region,
                    &json_bytes,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn generate_payment_report(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<ReportRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GeneratePaymentReport;
        Box::pin(api::server_wrap(
            flow,
            state.clone(),
            &req,
            json_payload.into_inner(),
            |state, (auth, user_id): auth::AuthenticationDataWithUserId, payload, _| async move {
                let user = UserInterface::find_user_by_id(&*state.global_store, &user_id)
                    .await
                    .change_context(AnalyticsError::UnknownError)?;

                let user_email = UserEmail::from_pii_email(user.email)
                    .change_context(AnalyticsError::UnknownError)?
                    .get_secret();

                let lambda_req = GenerateReportRequest {
                    request: payload,
                    merchant_id: auth.merchant_account.merchant_id.to_string(),
                    email: user_email,
                };

                let json_bytes =
                    serde_json::to_vec(&lambda_req).map_err(|_| AnalyticsError::UnknownError)?;
                invoke_lambda(
                    &state.conf.report_download_config.payment_function,
                    &state.conf.report_download_config.region,
                    &json_bytes,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::PaymentWrite),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetApiEventMetricRequest` element.
    pub async fn get_api_events_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetApiEventMetricRequest; 1]>,
    ) -> impl Responder {
        // safety: This shouldn't panic owing to the data type
        #[allow(clippy::expect_used)]
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetApiEventMetricRequest");
        let flow = AnalyticsFlow::GetApiEventMetrics;
        Box::pin(api::server_wrap(
            flow,
            state.clone(),
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::api_event::get_api_event_metrics(
                    &state.pool,
                    &auth.merchant_account.merchant_id,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_api_event_filters(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetApiEventFiltersRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetApiEventFilters;
        Box::pin(api::server_wrap(
            flow,
            state.clone(),
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                analytics::api_event::get_filters(
                    &state.pool,
                    req,
                    auth.merchant_account.merchant_id,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_connector_events(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Query<api_models::analytics::connector_events::ConnectorEventsRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetConnectorEvents;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                connector_events_core(&state.pool, req, auth.merchant_account.merchant_id)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_global_search_results(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetGlobalSearchRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetGlobalSearchResults;
        Box::pin(api::server_wrap(
            flow,
            state.clone(),
            &req,
            json_payload.into_inner(),
            |state, auth: UserFromToken, req, _| async move {
                let role_id = auth.role_id;
                let role_info =
                    RoleInfo::from_role_id(&state, &role_id, &auth.merchant_id, &auth.org_id)
                        .await
                        .change_context(UserErrors::InternalServerError)
                        .change_context(OpenSearchError::UnknownError)?;
                let permissions = role_info.get_permissions_set();
                let accessible_indexes: Vec<_> = OPENSEARCH_INDEX_PERMISSIONS
                    .iter()
                    .filter(|(_, perm)| perm.iter().any(|p| permissions.contains(p)))
                    .map(|(i, _)| *i)
                    .collect();

                analytics::search::msearch_results(
                    &state.opensearch_client,
                    req,
                    &auth.merchant_id,
                    accessible_indexes,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_search_results(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<GetSearchRequest>,
        index: web::Path<SearchIndex>,
    ) -> impl Responder {
        let index = index.into_inner();
        let flow = AnalyticsFlow::GetSearchResults;
        let indexed_req = GetSearchRequestWithIndex {
            search_req: json_payload.into_inner(),
            index,
        };
        Box::pin(api::server_wrap(
            flow,
            state.clone(),
            &req,
            indexed_req,
            |state, auth: UserFromToken, req, _| async move {
                let role_id = auth.role_id;
                let role_info =
                    RoleInfo::from_role_id(&state, &role_id, &auth.merchant_id, &auth.org_id)
                        .await
                        .change_context(UserErrors::InternalServerError)
                        .change_context(OpenSearchError::UnknownError)?;
                let permissions = role_info.get_permissions_set();
                let _ = OPENSEARCH_INDEX_PERMISSIONS
                    .iter()
                    .filter(|(ind, _)| *ind == index)
                    .find(|i| i.1.iter().any(|p| permissions.contains(p)))
                    .ok_or(OpenSearchError::IndexAccessNotPermittedError(index))?;
                analytics::search::search_results(&state.opensearch_client, req, &auth.merchant_id)
                    .await
                    .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }

    pub async fn get_dispute_filters(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<api_models::analytics::GetDisputeFilterRequest>,
    ) -> impl Responder {
        let flow = AnalyticsFlow::GetDisputeFilters;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            json_payload.into_inner(),
            |state, auth: AuthenticationData, req, _| async move {
                analytics::disputes::get_filters(
                    &state.pool,
                    req,
                    &auth.merchant_account.merchant_id,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }
    /// # Panics
    ///
    /// Panics if `json_payload` array does not contain one `GetDisputeMetricRequest` element.
    pub async fn get_dispute_metrics(
        state: web::Data<AppState>,
        req: actix_web::HttpRequest,
        json_payload: web::Json<[GetDisputeMetricRequest; 1]>,
    ) -> impl Responder {
        // safety: This shouldn't panic owing to the data type
        #[allow(clippy::expect_used)]
        let payload = json_payload
            .into_inner()
            .to_vec()
            .pop()
            .expect("Couldn't get GetDisputeMetricRequest");
        let flow = AnalyticsFlow::GetDisputeMetrics;
        Box::pin(api::server_wrap(
            flow,
            state,
            &req,
            payload,
            |state, auth: AuthenticationData, req, _| async move {
                analytics::disputes::get_metrics(
                    &state.pool,
                    &auth.merchant_account.merchant_id,
                    req,
                )
                .await
                .map(ApplicationResponse::Json)
            },
            &auth::JWTAuth(Permission::Analytics),
            api_locking::LockAction::NotApplicable,
        ))
        .await
    }
}
