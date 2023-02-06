use base64::Engine;
use common_utils::ext_traits::{Encode, ValueExt};
use error_stack::ResultExt;
use serde::{Deserialize, Serialize};

use crate::{
    core::errors,
    consts,
    pii::PeekInterface,
    types::{self, api, storage::enums},
    utils::OptionExt,
};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub enum TransactionType {
    #[serde(rename = "authCaptureTransaction")]
    Payment,
    #[serde(rename = "authOnlyTransaction")]
    PaymentAuthOnly,
    #[serde(rename = "priorAuthCaptureTransaction")]
    Capture,
    #[serde(rename = "refundTransaction")]
    Refund,
    #[serde(rename = "voidTransaction")]
    Void,
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct MerchantAuthentication {
    name: String,
    transaction_key: String,
}

impl TryFrom<&types::ConnectorAuthType> for MerchantAuthentication {
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(auth_type: &types::ConnectorAuthType) -> Result<Self, Self::Error> {
        if let types::ConnectorAuthType::BodyKey { api_key, key1 } = auth_type {
            Ok(Self {
                name: key1.clone(),
                transaction_key: api_key.clone(),
            })
        } else {
            Err(errors::ConnectorError::FailedToObtainAuthType)?
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct CreditCardDetails {
    card_number: masking::Secret<String, common_utils::pii::CardNumber>,
    expiration_date: masking::Secret<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    card_code: Option<masking::Secret<String>>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct BankAccountDetails {
    account_number: masking::Secret<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct WalletDetails {
    data_descriptor: String,
    data_value: String,
}


#[derive(Serialize, Deserialize, PartialEq, Debug)]
enum PaymentDetails {
    #[serde(rename = "creditCard")]
    CreditCard(CreditCardDetails),
    #[serde(rename = "bankAccount")]
    BankAccount(BankAccountDetails),
    #[serde(rename = "opaqueData")]
    Wallet(WalletDetails),
    Klarna,
    Paypal,
}

impl TryFrom<api_models::payments::PaymentMethod> for PaymentDetails {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(value: api_models::payments::PaymentMethod) -> Result<Self, Self::Error> {
        match value {
            api::PaymentMethod::Card(ref ccard) => {
                let expiry_month = ccard.card_exp_month.peek().clone();
                let expiry_year = ccard.card_exp_year.peek().clone();

                Ok(Self::CreditCard(CreditCardDetails {
                    card_number: ccard.card_number.clone(),
                    expiration_date: format!("{expiry_year}-{expiry_month}").into(),
                    card_code: Some(ccard.card_cvc.clone()),
                }))
            }
            api::PaymentMethod::BankTransfer => Ok(Self::BankAccount(BankAccountDetails {
                account_number: "XXXXX".to_string().into(),
            })),
            api::PaymentMethod::PayLater(_) => Ok(Self::Klarna),
            api::PaymentMethod::Wallet(wallet_data) => match wallet_data.issuer_name{
                api_models::enums::WalletIssuer::GooglePay => Ok(Self::Wallet(WalletDetails { 
                    data_descriptor: "COMMON.GOOGLE.INAPP.PAYMENT".to_string(),
                    data_value: consts::BASE64_ENGINE.encode(
                        wallet_data
                            .token
                            .get_required_value("token")
                            .change_context(errors::ConnectorError::RequestEncodingFailed)
                            .attach_printable("No token passed")?,
                    ),
                })),
                _ => Err(errors::ConnectorError::NotImplemented(
                    "Unknown Wallet in Payment Method".to_string(),
                ))?,
            },
            api::PaymentMethod::Paypal => Ok(Self::Paypal),
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(untagged)]
pub enum  TransactionRequest {
    AuthRequest(PaymentRequestData),
    CaptureRequest(CaptureRequestData),
    VoidRequest(VoidRequestData),
    RefundRequest(RefundRequestData),
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequestData {
    transaction_type: TransactionType,
    amount: i64,
    currency_code: String,
    payment: PaymentDetails,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRequestData {
    transaction_type: TransactionType,
    amount: i64,
    ref_trans_id: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AuthorizationIndicator {
    authorization_indicator: AuthorizationType,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VoidRequestData {
    transaction_type: TransactionType,
    ref_trans_id: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedotnetPaymentsRequest {
    merchant_authentication: MerchantAuthentication,
    transaction_request: TransactionRequest,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
// The connector enforces field ordering, it expects fields to be in the same order as in their API documentation
pub struct CreateTransactionRequest {
    create_transaction_request: AuthorizedotnetPaymentsRequest,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthorizationType {
    Final,
    Pre,
}

impl From<enums::CaptureMethod> for AuthorizationType {
    fn from(item: enums::CaptureMethod) -> Self {
        match item {
            enums::CaptureMethod::Manual => Self::Pre,
            _ => Self::Final,
        }
    }
}

impl TryFrom<&types::PaymentsAuthorizeRouterData> for CreateTransactionRequest {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(item: &types::PaymentsAuthorizeRouterData) -> Result<Self, Self::Error> {
        let payment_details = PaymentDetails::try_from(item.request.payment_method_data.clone())?;
        let transaction_type = match item.request.capture_method {
            Some(enums::CaptureMethod::Manual) =>  TransactionType::PaymentAuthOnly,
            _ => TransactionType::Payment
        };
        let transaction_request = TransactionRequest::AuthRequest(PaymentRequestData{
            transaction_type,
            amount: item.request.amount,
            payment: payment_details,
            currency_code: item.request.currency.to_string(),
        });

        let merchant_authentication = MerchantAuthentication::try_from(&item.connector_auth_type)?;

        Ok(Self {
            create_transaction_request: AuthorizedotnetPaymentsRequest {
                merchant_authentication,
                transaction_request,
            },
        })
    }
}

impl TryFrom<&types::PaymentsCaptureRouterData> for CreateTransactionRequest {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(item: &types::PaymentsCaptureRouterData) -> Result<Self, Self::Error> {
        let transaction_type = TransactionType::Capture;
        let transaction_request = TransactionRequest::CaptureRequest(CaptureRequestData{
            transaction_type,
            amount: item.request.amount,
            ref_trans_id: item.request.connector_transaction_id.to_string(),
        });

        let merchant_authentication = MerchantAuthentication::try_from(&item.connector_auth_type)?;

        Ok(Self {
            create_transaction_request: AuthorizedotnetPaymentsRequest {
                merchant_authentication,
                transaction_request,
            },
        })
    }
}


impl TryFrom<&types::PaymentsCancelRouterData> for CreateTransactionRequest {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(item: &types::PaymentsCancelRouterData) -> Result<Self, Self::Error> {
        let transaction_request = TransactionRequest::VoidRequest(VoidRequestData{
            transaction_type: TransactionType::Void,
            ref_trans_id: item.request.connector_transaction_id.to_string(),
        });

        let merchant_authentication = MerchantAuthentication::try_from(&item.connector_auth_type)?;

        Ok(Self {
            create_transaction_request: AuthorizedotnetPaymentsRequest {
                merchant_authentication,
                transaction_request,
            },
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub enum AuthorizedotnetPaymentStatus {
    #[serde(rename = "1")]
    Approved,
    #[serde(rename = "2")]
    Declined,
    #[serde(rename = "3")]
    Error,
    #[serde(rename = "4")]
    #[default]
    HeldForReview,
}

pub type AuthorizedotnetRefundStatus = AuthorizedotnetPaymentStatus;

impl From<AuthorizedotnetPaymentStatus> for enums::AttemptStatus {
    fn from(item: AuthorizedotnetPaymentStatus) -> Self {
        match item {
            AuthorizedotnetPaymentStatus::Approved => Self::Charged,
            AuthorizedotnetPaymentStatus::Declined | AuthorizedotnetPaymentStatus::Error => {
                Self::Failure
            }
            AuthorizedotnetPaymentStatus::HeldForReview => Self::Pending,
        }
    }
}

fn get_payment_status(is_auth_only: bool, status: enums::AttemptStatus) -> enums::AttemptStatus {
    let is_authorized = matches!(status, enums::AttemptStatus::Charged);
    if is_auth_only && is_authorized {
        return enums::AttemptStatus::Authorized;
    }
    status
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct ResponseMessage {
    code: String,
    text: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
enum ResultCode {
    Ok,
    Error,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMessages {
    result_code: ResultCode,
    message: Vec<ResponseMessage>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct ErrorMessage {
    pub(super) error_code: String,
    pub(super) error_text: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    response_code: AuthorizedotnetPaymentStatus,
    auth_code: String,
    #[serde(rename = "transId")]
    transaction_id: String,
    pub(super) account_number: Option<String>,
    pub(super) errors: Option<Vec<ErrorMessage>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedotnetPaymentsResponse {
    pub transaction_response: TransactionResponse,
    pub messages: ResponseMessages,
}

impl<F, T>
    TryFrom<
        (types::ResponseRouterData<
            F,
            AuthorizedotnetPaymentsResponse,
            T,
            types::PaymentsResponseData,
        >,
        bool,
    )> for types::RouterData<F, T, types::PaymentsResponseData>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        data: (types::ResponseRouterData<
            F,
            AuthorizedotnetPaymentsResponse,
            T,
            types::PaymentsResponseData,
        >,bool),
    ) -> Result<Self, Self::Error> {
        let item = data.0;
        let status = enums::AttemptStatus::from(item.response.transaction_response.response_code);
        let error = item
            .response
            .transaction_response
            .errors
            .and_then(|errors| {
                errors.into_iter().next().map(|error| types::ErrorResponse {
                    code: error.error_code,
                    message: error.error_text,
                    reason: None,
                    status_code: item.http_code,
                })
            });

        let metadata = item
            .response
            .transaction_response
            .account_number
            .map(|acc_no| {
                Encode::<'_, PaymentDetails>::encode_to_value(&construct_refund_payment_details(
                    acc_no,
                ))
            })
            .transpose()
            .change_context(errors::ConnectorError::MissingRequiredField {
                field_name: "connector_metadata",
            })?;
        let is_auth_only = data.1;
        Ok(Self {
            status: get_payment_status(is_auth_only, status),
            response: match error {
                Some(err) => Err(err),
                None => Ok(types::PaymentsResponseData::TransactionResponse {
                    resource_id: types::ResponseId::ConnectorTransactionId(
                        item.response.transaction_response.transaction_id,
                    ),
                    redirection_data: None,
                    redirect: false,
                    mandate_reference: None,
                    connector_metadata: metadata,
                }),
            },
            ..item.data
        })
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RefundRequestData {
    transaction_type: TransactionType,
    amount: i64,
    currency_code: String,
    payment: PaymentDetails,
    #[serde(rename = "refTransId")]
    reference_transaction_id: String,
}

impl<F> TryFrom<&types::RefundsRouterData<F>> for CreateTransactionRequest {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(item: &types::RefundsRouterData<F>) -> Result<Self, Self::Error> {
        let payment_details = item
            .request
            .connector_metadata
            .as_ref()
            .get_required_value("connector_metadata")
            .change_context(errors::ConnectorError::MissingRequiredField {
                field_name: "connector_metadata",
            })?
            .clone();

        let merchant_authentication = MerchantAuthentication::try_from(&item.connector_auth_type)?;

        let transaction_request = TransactionRequest::RefundRequest(RefundRequestData{
            transaction_type: TransactionType::Refund,
            amount: item.request.refund_amount,
            payment: payment_details
                .parse_value("PaymentDetails")
                .change_context(errors::ConnectorError::MissingRequiredField {
                    field_name: "payment_details",
                })?,
            currency_code: item.request.currency.to_string(),
            reference_transaction_id: item.request.connector_transaction_id.clone(),
        });

        Ok(Self {
            create_transaction_request: AuthorizedotnetPaymentsRequest {
                merchant_authentication,
                transaction_request,
            },
        })
    }
}

impl From<AuthorizedotnetPaymentStatus> for enums::RefundStatus {
    fn from(item: AuthorizedotnetRefundStatus) -> Self {
        match item {
            AuthorizedotnetPaymentStatus::Approved => Self::Success,
            AuthorizedotnetPaymentStatus::Declined | AuthorizedotnetPaymentStatus::Error => {
                Self::Failure
            }
            AuthorizedotnetPaymentStatus::HeldForReview => Self::Pending,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedotnetRefundResponse {
    pub transaction_response: TransactionResponse,
    pub messages: ResponseMessages,
}

impl<F> TryFrom<types::RefundsResponseRouterData<F, AuthorizedotnetRefundResponse>>
    for types::RefundsRouterData<F>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::RefundsResponseRouterData<F, AuthorizedotnetRefundResponse>,
    ) -> Result<Self, Self::Error> {
        let transaction_response = &item.response.transaction_response;
        let refund_status = enums::RefundStatus::from(transaction_response.response_code.clone());
        let error = transaction_response.errors.clone().and_then(|errors| {
            errors.first().map(|error| types::ErrorResponse {
                code: error.error_code.clone(),
                message: error.error_text.clone(),
                reason: None,
                status_code: item.http_code,
            })
        });

        Ok(Self {
            response: match error {
                Some(err) => Err(err),
                None => Ok(types::RefundsResponseData {
                    connector_refund_id: transaction_response.transaction_id.clone(),
                    refund_status,
                }),
            },
            ..item.data
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetails {
    merchant_authentication: MerchantAuthentication,
    #[serde(rename = "transId")]
    transaction_id: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedotnetCreateSyncRequest {
    get_transaction_details_request: TransactionDetails,
}

impl<F> TryFrom<&types::RefundsRouterData<F>> for AuthorizedotnetCreateSyncRequest {
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(item: &types::RefundsRouterData<F>) -> Result<Self, Self::Error> {
        let transaction_id = item
            .request
            .connector_refund_id
            .clone()
            .get_required_value("connector_refund_id")
            .change_context(errors::ConnectorError::MissingConnectorRefundID)?;
        let merchant_authentication = MerchantAuthentication::try_from(&item.connector_auth_type)?;

        let payload = Self {
            get_transaction_details_request: TransactionDetails {
                merchant_authentication,
                transaction_id: Some(transaction_id),
            },
        };
        Ok(payload)
    }
}

impl TryFrom<&types::PaymentsSyncRouterData> for AuthorizedotnetCreateSyncRequest {
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(item: &types::PaymentsSyncRouterData) -> Result<Self, Self::Error> {
        let transaction_id = item
            .response
            .as_ref()
            .ok()
            .map(|payment_response_data| match payment_response_data {
                types::PaymentsResponseData::TransactionResponse { resource_id, .. } => {
                    resource_id.get_connector_transaction_id()
                }
                _ => Err(error_stack::report!(
                    errors::ValidationError::MissingRequiredField {
                        field_name: "transaction_id".to_string()
                    }
                )),
            })
            .transpose()
            .change_context(errors::ConnectorError::ResponseHandlingFailed)?;

        let merchant_authentication = MerchantAuthentication::try_from(&item.connector_auth_type)?;

        let payload = Self {
            get_transaction_details_request: TransactionDetails {
                merchant_authentication,
                transaction_id,
            },
        };
        Ok(payload)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncStatus {
    RefundSettledSuccessfully,
    RefundPendingSettlement,
    AuthorizedPendingCapture,
    CapturedPendingSettlement,
    SettledSuccessfully,
    Declined,
    Voided,
    CouldNotVoid,
    GeneralError,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncTransactionResponse {
    #[serde(rename = "transId")]
    transaction_id: String,
    transaction_status: SyncStatus,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizedotnetSyncResponse {
    transaction: SyncTransactionResponse,
}

impl From<SyncStatus> for enums::RefundStatus {
    fn from(transaction_status: SyncStatus) -> Self {
        match transaction_status {
            SyncStatus::RefundSettledSuccessfully => Self::Success,
            SyncStatus::RefundPendingSettlement => Self::Pending,
            _ => Self::Failure,
        }
    }
}

impl From<SyncStatus> for enums::AttemptStatus {
    fn from(transaction_status: SyncStatus) -> Self {
        match transaction_status {
            SyncStatus::SettledSuccessfully | SyncStatus::CapturedPendingSettlement => {
                Self::Charged
            }
            SyncStatus::Declined => Self::AuthenticationFailed,
            SyncStatus::Voided => Self::Voided,
            SyncStatus::CouldNotVoid => Self::VoidFailed,
            SyncStatus::GeneralError => Self::Failure,
            _ => Self::Pending,
        }
    }
}

impl TryFrom<types::RefundsResponseRouterData<api::RSync, AuthorizedotnetSyncResponse>>
    for types::RefundsRouterData<api::RSync>
{
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(
        item: types::RefundsResponseRouterData<api::RSync, AuthorizedotnetSyncResponse>,
    ) -> Result<Self, Self::Error> {
        let refund_status = enums::RefundStatus::from(item.response.transaction.transaction_status);
        Ok(Self {
            response: Ok(types::RefundsResponseData {
                connector_refund_id: item.response.transaction.transaction_id.clone(),
                refund_status,
            }),
            ..item.data
        })
    }
}

impl<F, Req>
    TryFrom<
        types::ResponseRouterData<F, AuthorizedotnetSyncResponse, Req, types::PaymentsResponseData>,
    > for types::RouterData<F, Req, types::PaymentsResponseData>
{
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(
        item: types::ResponseRouterData<
            F,
            AuthorizedotnetSyncResponse,
            Req,
            types::PaymentsResponseData,
        >,
    ) -> Result<Self, Self::Error> {
        let payment_status =
            enums::AttemptStatus::from(item.response.transaction.transaction_status);
        Ok(Self {
            response: Ok(types::PaymentsResponseData::TransactionResponse {
                resource_id: types::ResponseId::ConnectorTransactionId(
                    item.response.transaction.transaction_id,
                ),
                redirection_data: None,
                redirect: false,
                mandate_reference: None,
                connector_metadata: None,
            }),
            status: payment_status,
            ..item.data
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedotnetWebhookObjectId{
    pub webhook_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedotnetWebhookEventType{
    pub event_type: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizedotnetWebhookObjectResource{
    pub data: serde_json::Value,
}
#[derive(Debug, Default, Eq, PartialEq, Deserialize)]
pub struct ErrorDetails {
    pub code: Option<String>,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub message: Option<String>,
    pub param: Option<String>,
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq)]
pub struct AuthorizedotnetErrorResponse {
    pub error: ErrorDetails,
}

fn construct_refund_payment_details(masked_number: String) -> PaymentDetails {
    PaymentDetails::CreditCard(CreditCardDetails {
        card_number: masked_number.into(),
        expiration_date: "XXXX".to_string().into(),
        card_code: None,
    })
}
