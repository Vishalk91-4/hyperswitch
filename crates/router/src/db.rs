pub mod address;
pub mod cache;
pub mod configs;
pub mod connector_response;
pub mod customers;
pub mod ephemeral_key;
pub mod events;
pub mod locker_mock_up;
pub mod mandate;
pub mod merchant_account;
pub mod merchant_connector_account;
pub mod payment_attempt;
pub mod payment_intent;
pub mod payment_method;
pub mod process_tracker;
pub mod queue;
pub mod refund;
pub mod reverse_lookup;

use std::sync::Arc;

use common_utils::errors::CustomResult;
use error_stack::report;
use futures::lock::Mutex;

use crate::{core::errors, services::Store, types::storage};

#[derive(PartialEq, Eq)]
pub enum StorageImpl {
    Postgresql,
    PostgresqlTest,
    Mock,
}

#[async_trait::async_trait]
pub trait StorageInterface:
    Send
    + Sync
    + dyn_clone::DynClone
    + payment_attempt::PaymentAttemptInterface
    + mandate::MandateInterface
    + address::AddressInterface
    + configs::ConfigInterface
    + customers::CustomerInterface
    + events::EventInterface
    + merchant_account::MerchantAccountInterface
    + merchant_connector_account::MerchantConnectorAccountInterface
    + merchant_connector_account::ConnectorAccessToken
    + locker_mock_up::LockerMockUpInterface
    + payment_intent::PaymentIntentInterface
    + payment_method::PaymentMethodInterface
    + process_tracker::ProcessTrackerInterface
    + refund::RefundInterface
    + queue::QueueInterface
    + ephemeral_key::EphemeralKeyInterface
    + connector_response::ConnectorResponseInterface
    + reverse_lookup::ReverseLookupInterface
    + 'static
    + InternalLoader
{
    async fn close(&mut self) {}
}

pub trait InternalLoader {
    fn get_store(&self) -> CustomResult<&Store, errors::StorageError> {
        Err(report!(errors::StorageError::DatabaseError(report!(
            storage_models::errors::DatabaseError::Others
        ))))
    }
    fn get_mock_db(&self) -> CustomResult<&MockDb, errors::StorageError> {
        Err(report!(errors::StorageError::MockDbError))
    }
}

impl InternalLoader for Store {
    fn get_store(&self) -> CustomResult<&Store, errors::StorageError> {
        Ok(self)
    }
}

impl InternalLoader for MockDb {
    fn get_mock_db(&self) -> CustomResult<&MockDb, errors::StorageError> {
        Ok(self)
    }
}

#[async_trait::async_trait]
impl StorageInterface for Store {
    #[allow(clippy::expect_used)]
    async fn close(&mut self) {
        std::sync::Arc::get_mut(&mut self.redis_conn)
            .expect("Redis connection pool cannot be closed")
            .close_connections()
            .await;
    }
}

#[derive(Clone)]
pub struct MockDb {
    merchant_accounts: Arc<Mutex<Vec<storage::MerchantAccount>>>,
    merchant_connector_accounts: Arc<Mutex<Vec<storage::MerchantConnectorAccount>>>,
    payment_attempts: Arc<Mutex<Vec<storage::PaymentAttempt>>>,
    payment_intents: Arc<Mutex<Vec<storage::PaymentIntent>>>,
    customers: Arc<Mutex<Vec<storage::Customer>>>,
    refunds: Arc<Mutex<Vec<storage::Refund>>>,
    processes: Arc<Mutex<Vec<storage::ProcessTracker>>>,
    connector_response: Arc<Mutex<Vec<storage::ConnectorResponse>>>,
    redis: Arc<redis_interface::RedisConnectionPool>,
}

impl MockDb {
    pub async fn new(redis: &crate::configs::settings::Settings) -> Self {
        Self {
            merchant_accounts: Default::default(),
            merchant_connector_accounts: Default::default(),
            payment_attempts: Default::default(),
            payment_intents: Default::default(),
            customers: Default::default(),
            refunds: Default::default(),
            processes: Default::default(),
            connector_response: Default::default(),
            redis: Arc::new(crate::connection::redis_connection(redis).await),
        }
    }
}

#[async_trait::async_trait]
impl StorageInterface for MockDb {
    #[allow(clippy::expect_used)]
    async fn close(&mut self) {
        std::sync::Arc::get_mut(&mut self.redis)
            .expect("Redis connection pool cannot be closed")
            .close_connections()
            .await;
    }
}

pub async fn get_and_deserialize_key<T>(
    db: &dyn StorageInterface,
    key: &str,
    type_name: &str,
) -> CustomResult<T, errors::RedisError>
where
    T: serde::de::DeserializeOwned,
{
    use common_utils::ext_traits::ByteSliceExt;
    use error_stack::ResultExt;

    let bytes = db.get_key(key).await?;
    bytes
        .parse_struct(type_name)
        .change_context(redis_interface::errors::RedisError::JsonDeserializationFailed)
}

dyn_clone::clone_trait_object!(StorageInterface);
